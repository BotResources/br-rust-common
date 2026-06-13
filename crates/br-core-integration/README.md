# br-core-integration

Typed envelopes and a publisher trait for **integration messaging** — the
events and commands that travel *between* bounded contexts on the message bus.

**Purpose.** Where [`br-core-events`](../br-core-events/) holds the shapes used
*inside* a context's event store, this crate holds the shapes used *between*
contexts: `IntegrationEvent<T>` (facts) and `IntegrationCommand<T>` (requests),
plus the `IntegrationPublisher` trait services use to emit them.

**When to use.** A service produces or consumes cross-context messages and
needs to agree with peers on the wire shape and metadata fields.

**When not to use.** In-context domain events — keep those in
`br-core-events`. Per-context concrete payload types (e.g.
`UserCreatedV1 { … }`) — those belong to the producing service.

## What's inside

| Type | Role |
|---|---|
| `EventMetadata` | Re-export of `br_core_events::EventMetadata` — one type, one wire contract. Carries a typed `br_core_kernel::Actor` (human or machine), `correlation_id`, `causation_id` (skipped on the wire when `None`). Backward-compatible wire format: pre-`Actor` payloads (no `actor_kind`) default to a human actor. |
| `IntegrationEvent<T>` | Envelope for a fact: `event_id`, `event_type`, `version: u8`, `occurred_at`, `metadata`, `payload: T`. `#[non_exhaustive]` — build via `IntegrationEvent::new`. |
| `IntegrationCommand<T>` | Envelope for a request: `command_id`, `command_type`, `version: u8`, `issued_at`, `metadata`, `payload: T`. `#[non_exhaustive]` — build via `IntegrationCommand::new`. |
| `IntegrationError` | `Publish { kind, detail }` (transport), `Consume { kind, detail }` (bind/pull), `Decode { subject, detail }` (poison message), `Serialization(serde_json::Error)` (encoding). `#[non_exhaustive]`. |
| `PublishErrorKind` | Classifies a publish failure: `NoStream`, `Timeout`, `Other`. `#[non_exhaustive]`. |
| `ConsumeErrorKind` | Classifies a consume/bind failure: `NoStream`, `NoConsumer` (missing declared object at bind), `ConsumerGone` (the consumer vanished mid-run — deleted server-side, or the awaiter's NATS connection closed), `Other`. `#[non_exhaustive]`. |
| `IntegrationPublisher` (trait, object-safe) | `publish(subject, payload) -> Result<(), IntegrationError>` and fire-and-forget `publish_if_connected(subject, payload)`. |
| `IntegrationPublisherExt` (blanket trait) | Typed helpers: `publish_event`, `publish_command`, and `_if_connected` variants. |
| `NatsIntegrationPublisher` | JetStream-backed implementation, awaits the broker ack. |
| `NoopIntegrationPublisher` | No-op; for tests and as a default when messaging is disabled. |
| `DurableConsumer` / `Delivery` / `MessageOutcome` | The **receiver** consumer shape: binds a durable consumer and runs a typed handler over a parked message stream (see [Consuming](#consuming)). |
| `CorrelatedAwaiter` / `CorrelatedMatch` | The **awaiter** consumer shape: a core NATS push subscription on the confirmation subjects that resolves on the first message matching a `correlation_id`. See [Consuming](#consuming). |
| `integration_subject` / `MessageKind` / `SubjectError` | Builds and validates the subject convention (see below). |
| `OutboxStatus` / `OutboxRecord` / `next_after_attempt` / `classify_failure` / `retry_backoff` / `verify_consumer` | The **pure** outbox core (no feature, no `sqlx`): the staged-message value, the retry state machine, the structural-vs-transient failure classifier, the retry-backoff policy, and the ungated receiver-online precheck (`async_nats` only). See [Transactional outbox](#transactional-outbox). |
| `OutboxStore` / `OutboxRelay` / `RelayPolicy` / `RelayHealth` | The outbox's Postgres store + subscribe-driven per-row-commit relay (`run` is the entry point), plus its `Degraded`-on-structural-failure health signal. Behind the **`outbox`** feature (pulls `sqlx`). |

## Subject naming convention

Subjects follow `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}`
(e.g. `identity.evt.user.created.v1`, `notifier.cmd.notification.send.v1`).
Build them with `integration_subject` rather than formatting strings by hand —
it is the single source of the convention and validates that each segment is
non-empty and drawn from `[a-z0-9_-]` (no `.`, no NATS wildcards, no
whitespace; multi-word segments are snake_case, e.g. `service_scope`):

```rust
use br_core_integration::{integration_subject, MessageKind};

let subject = integration_subject("identity", MessageKind::Evt, "user", "created", 1).unwrap();
assert_eq!(subject, "identity.evt.user.created.v1");
```

Subscribers use NATS wildcards (`identity.evt.>`, `notifier.cmd.>`) to consume
relevant streams.

## Usage

```rust
use std::sync::Arc;
use br_core_integration::{
    integration_subject, IntegrationEvent, IntegrationPublisher,
    IntegrationPublisherExt, MessageKind, EventMetadata, NatsIntegrationPublisher,
};
use br_core_integration::{Actor, UserId};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct UserCreatedV1 { user_id: Uuid, email: String }

# async fn example(jetstream: async_nats::jetstream::Context) -> Result<(), Box<dyn std::error::Error>> {
let publisher: Arc<dyn IntegrationPublisher> =
    Arc::new(NatsIntegrationPublisher::new(jetstream));

let metadata = EventMetadata::new(
    Actor::Human(UserId::from(Uuid::now_v7())),
    Uuid::now_v7(), // correlation_id
);

let evt = IntegrationEvent::new(
    Uuid::now_v7(), // event_id
    "user.created",
    1,
    Utc::now(),
    metadata,
    UserCreatedV1 {
        user_id: Uuid::now_v7(),
        email: "alice@example.com".into(),
    },
);

let subject = integration_subject("identity", MessageKind::Evt, "user", "created", 1)?;
publisher.publish_event(&subject, &evt).await?;
# Ok(()) }
```

For best-effort emission where the request must not fail because the bus is
down, use `publish_event_if_connected` / `publish_command_if_connected`.

## Consuming

Receiving comes in **two deliberately different shapes**. Pick by role.

| Shape | Type | Use when | Consumer | Queue group? |
|---|---|---|---|---|
| **Receiver** | `DurableConsumer` | You consume commands/events addressed to your context (e.g. Identity consuming a `…cmd…` subject). | A *pre-declared* durable consumer, bound by name. | Yes — replicas binding the **same** durable name share delivery (one message → one worker). |
| **Awaiter** | `CorrelatedAwaiter` | You published a command and await its correlated reply (e.g. a declaring replica awaiting `…accepted` / `…rejected`). | A *per-boot* core NATS push subscription, opened at runtime. | No — every replica sees all replies and filters its own by `correlation_id`. |

**No auto-provisioning.** Both assert the declared stream exists by name and
**fail loud** if it — or, for `DurableConsumer`, the named consumer — is missing
(`IntegrationError::Consume` with `ConsumeErrorKind::NoStream` / `NoConsumer`).
The lib never creates a stream or a durable consumer. The awaiter still asserts
the stream (the declared infra must exist) but awaits over a **core NATS push
subscription**, not a JetStream consumer — no infrastructure is created.

**Delivery is at-least-once, not exactly-once.** A message is redelivered until
explicitly acked or termed; the handler returns a `MessageOutcome`
(`Ack` / `Nak(delay)` / `Term`). For effective once-only processing, make the
handler **idempotent** (de-dupe on the envelope id) — the transport does not
provide it. `Nak(None)` redelivers at the consumer's server-configured `AckWait`
(not immediately), and repeated naks redeliver at that same cadence; prefer
`Nak(Some(delay))` to back off explicitly.

**No graceful drain (0.3.0).** `run_commands` / `run_events` own the stream until
it ends or a fatal transport error occurs — there is no cooperative shutdown.
Stopping a consumer means aborting its task; a message **in flight at abort** is
neither acked nor naked, so JetStream redelivers it after `AckWait`
(at-least-once covers correctness — expect some redelivery latency on rollouts).
A `CancellationToken`-style drain is a planned **additive** addition.

**Poison messages fail closed.** A payload that does not deserialize into the
expected typed envelope is `term`ed (so it is not redelivered forever) and
surfaced through `on_poison` as `IntegrationError::Decode` — never a silent
drop, never an infinite redelivery loop.

### Receiver — `DurableConsumer`

```rust
use br_core_integration::{Delivery, DurableConsumer, IntegrationCommand, MessageOutcome};
use serde::Deserialize;

#[derive(Deserialize)]
struct DeclareScopesV1 { service: String }

# async fn example(jetstream: async_nats::jetstream::Context) -> Result<(), Box<dyn std::error::Error>> {
let consumer = DurableConsumer::bind(&jetstream, "IDENTITY", "service_scope_declare").await?;
consumer
    .run_commands(
        |d: Delivery<IntegrationCommand<DeclareScopesV1>>| async move {
            // … run the domain command for d.envelope.payload …
            MessageOutcome::Ack
        },
        |poison| tracing::error!(error = %poison, "poison message termed"),
    )
    .await?;
# Ok(()) }
```

### Awaiter — `CorrelatedAwaiter`

The confirmation is a transient, fire-once correlated reply — it needs neither
durability nor catch-up — so the awaiter is a **core NATS push subscription** on
the confirmation subjects, not a JetStream consumer. `create` first asserts the
declared stream exists (fail loud with `ConsumeErrorKind::NoStream` if absent —
the lib never auto-provisions), then opens the core subscriptions over the
JetStream context's own `client()`. A JetStream publish also reaches core
subscribers on the same subject, so the JetStream-published reply is delivered
live.

The protocol is **subscribe-first**: `create` opens the subscriptions, *then* you
publish the command, then you await. Because the subscription is established
before the command is published, the reply always arrives after the SUB — a reply
landing *inside* the first await window is delivered within that window, with no
consumer-establishment race. On timeout, re-publish (same `correlation_id`) and
await again; a core subscription parks at zero CPU between waits and issues no
pull requests, so it stays armed indefinitely with no inactivity bound. A reply
emitted *before* the awaiter exists is missed by design; subscribe-first +
re-publish makes that safe.

```rust
use std::time::Duration;
use br_core_integration::CorrelatedAwaiter;
use uuid::Uuid;

# async fn example(jetstream: async_nats::jetstream::Context) -> Result<(), Box<dyn std::error::Error>> {
let correlation_id = Uuid::now_v7();
let mut awaiter = CorrelatedAwaiter::create(
    &jetstream,
    "IDENTITY",
    vec![
        "identity.evt.service_scope.accepted.v1".to_string(),
        "identity.evt.service_scope.rejected.v1".to_string(),
    ],
)
.await?;

// … publish the command carrying `correlation_id` here (subscribe-first) …

if let Some(m) = awaiter.await_correlation(correlation_id, Duration::from_secs(5)).await? {
    // `m.subject` tells you which payload type to decode (accepted vs rejected).
    println!("confirmation on {}", m.subject);
}
# Ok(()) }
```

## Transactional outbox

A critical integration event published best-effort *after* a commit can be lost
in the window between the domain commit and the bus publish (a crash, a broker
blip) — and a lost event in a choreography is the hardest bug to diagnose,
because nothing errors: the producer succeeded, the consumer simply never heard.
The **outbox** closes that window: the message becomes durable *atomically* with
the state it announces, and a relay guarantees it is eventually published.

Two phases:

1. **Stage** (in the caller's transaction): `stage` inserts an `OutboxRecord`
   row through the caller's executor (`&mut *tx`), so it commits *with* the
   domain write — atomic. Idempotent on the row id (`ON CONFLICT (id) DO
   NOTHING`); the id is a creator-supplied UUIDv7. In the **same transaction** it
   fires `pg_notify` on the table's channel — the wake that drives the relay (see
   below). Postgres delivers that `NOTIFY` **only at commit**, and **never** on
   rollback, so a rolled-back write never wakes the relay (the same
   notify-after-commit guarantee `br-util-broadcast` relies on).
2. **Relay** (subscribe-driven, and the crash-recovery sweep): `OutboxRelay::run`
   owns its loop. On entry it does **one** startup recovery drain (rows a crash
   left `Pending`), then parks on a `tokio::select!` that wakes only on a real
   event — a `NOTIFY` (a row was committed), a `LISTEN` **reconnect** (covers a
   `NOTIFY` that could be missed while the socket was down), or a chained
   **retry deadline** (present only when a transient failure owes a retry) — and
   **never on a blind timer**. When the outbox is clean and no retry is owed it
   is parked at **zero CPU** with **zero DB traffic** until the next `NOTIFY`
   (BR's never-poll rule). Each pass drains via `run_once` — processing `Pending`
   rows **one at a time, each in its own short transaction** (`BEGIN; SELECT …
   WHERE id > cursor FOR UPDATE SKIP LOCKED LIMIT 1; publish; apply_transition;
   COMMIT`), looping until none remain (or `max_messages`). `FOR UPDATE SKIP
   LOCKED` lets replicas drain disjoint rows. The *same* code that publishes
   right after a commit re-publishes a row a crash left `Pending` — there is no
   separate recovery path to forget.

   **Why per-row, not per-batch:** the publish IO is never held inside a
   transaction that locks a whole batch. A slow broker pins only the one row
   being published (and its connection), not 64 rows for the sum of 64 network
   round-trips; and a DB error on one row's transition rolls back *that row only*
   — never dozens of already-acked siblings. A per-pass `id` cursor makes each
   row attempted at most once per pass, so the pass never spins on a failing row.

   `run_once` itself stays `pub` — it is the single drain-until-empty building
   block, useful for a test or a manual operator recovery sweep — but `run` is
   the entry point in a service; do **not** call `run_once` on a timer.

**Structural vs transient failures + the health signal.** A publish that fails
because the target JetStream stream is **not declared** (`NoStream`) is an
infra-declaration fault, not a delivery attempt: the row stays `Pending` and
**does not consume an attempt** against `max_attempts`, so a misconfiguration
never marches a row to `Failed`. The relay flips its health to
`RelayHealth::Degraded { reason }` (a stable, language-free reason **code**); a
later structural-free pass restores `Healthy`. A **transient** failure (timeout,
broker blip) counts an attempt, stays `Pending` until the cap (`Failed`), and
arms the chained retry deadline.

`OutboxRelay::health()` returns a `tokio::sync::watch::Receiver<RelayHealth>`.
Because `br-core-integration` is a `core` crate it must **not** depend on
`br-util-axum-readiness` (a `util` crate — the tier rule), so it exposes the raw
signal and the **consuming service** bridges it into its readiness gate:

```rust,ignore
// In the service's readiness handler — map Degraded → 503.
let mut health = relay.health();        // read BEFORE spawning run()
tokio::spawn(async move { relay.run(shutdown_rx).await });
// …
let ready = matches!(*health.borrow_and_update(), RelayHealth::Healthy);
```

**Feature-gated.** The pure core (`OutboxStatus`, `OutboxRecord`,
`next_after_attempt`) and the ungated `verify_consumer` precheck are always
available and DB-free. The Postgres store + relay are behind the **`outbox`**
feature, which pulls `sqlx` — enable it only in a service that stages outbox rows
in its own Postgres transaction.

**Semantics — at-least-once, post-commit.** Publish happens after the staging
transaction commits, so a consumer never dirty-reads an uncommitted producer
write. A crash after the broker ack but before the status update leaves the row
`Pending`, and the next pass re-publishes it — so **subscribers must de-dupe on
the envelope id** (the same idempotency rule the consumer shapes document). There
is no exactly-once.

**Publisher timeout.** Under per-row commit a hung publish still holds one row's
lock and one connection until it returns — a bounded blast radius, but the
publish should still be bounded in time. The timeout belongs on the
`IntegrationPublisher` (`NatsIntegrationPublisher`), so every publish path — relay,
direct, fire-and-forget — inherits it; a timed-out publish then surfaces as a
normal failed attempt and the row stays `Pending` for the next pass.

**The table is a declared object — the lib never auto-provisions.** The store
assumes the table already exists; the consuming service's migrations own it. The
`NOTIFY` wake needs **no extra column** — the channel is derived from the table
name and the notify is fired by `pg_notify` in the staging query. The canonical
DDL the store binds to:

```sql
CREATE TABLE integration_outbox (
    id           UUID        PRIMARY KEY,            -- UUIDv7, creator-supplied
    subject      TEXT        NOT NULL,
    payload      JSONB       NOT NULL,
    status       TEXT        NOT NULL DEFAULT 'PENDING',
    attempts     BIGINT      NOT NULL DEFAULT 0,
    last_error   TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    published_at TIMESTAMPTZ
);
CREATE INDEX integration_outbox_pending_idx
    ON integration_outbox (id) WHERE status = 'PENDING';
```

```rust,ignore
use std::sync::Arc;
use br_core_integration::outbox::stage;
use br_core_integration::{
    integration_subject, IntegrationEvent, IntegrationPublisher, MessageKind,
    NatsIntegrationPublisher, OutboxRecord, OutboxRelay,
};
use uuid::Uuid;

// 1) Stage in the SAME transaction as the domain write — the commit fires the
//    NOTIFY that wakes the relay (and never fires on rollback).
let subject = integration_subject("identity", MessageKind::Evt, "user", "created", 1)?;
let record = OutboxRecord::stage_event(Uuid::now_v7(), &subject, &event)?;
let mut tx = pool.begin().await?;
// … the domain write on `&mut *tx` …
stage(&mut *tx, &record).await?;
tx.commit().await?;

// 2) Run the relay ONCE per service, subscribe-driven: it does a startup
//    recovery drain, then parks on NOTIFY / reconnect / retry — never a timer.
let publisher: Arc<dyn IntegrationPublisher> = Arc::new(NatsIntegrationPublisher::new(jetstream));
let relay = OutboxRelay::new(pool.clone(), publisher);
let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
let relay_health = relay.health();           // bridge into readiness (see above)
tokio::spawn(async move { relay.run(shutdown_rx).await });
// … on graceful shutdown: shutdown_tx.send(true);
```

For a command whose receiver must be online before issuing it, `verify_consumer`
is an opt-in fail-fast precheck (`ConsumeErrorKind::NoConsumer` when no durable
consumer is bound) — separate from the relay, and it never auto-provisions.

## Install

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-integration = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-integration", tag = "v0.10.0" }

# The transactional outbox (the `OutboxStore` + `OutboxRelay`) is behind an
# opt-in feature that pulls `sqlx`; the base crate stays DB-free without it:
# br-core-integration = { git = "...", package = "br-core-integration", tag = "v0.10.0", features = ["outbox"] }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
