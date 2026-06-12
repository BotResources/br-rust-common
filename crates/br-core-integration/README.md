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
| `MessageMetadata` | Re-export of `br_core_events::EventMetadata` — one type, one wire contract. Carries a typed `br_core_kernel::Actor` (human or machine), `correlation_id`, `causation_id` (skipped on the wire when `None`). Backward-compatible wire format: pre-`Actor` payloads (no `actor_kind`) default to a human actor. |
| `IntegrationEvent<T>` | Envelope for a fact: `event_id`, `event_type`, `version: u8`, `occurred_at`, `metadata`, `payload: T`. `#[non_exhaustive]` — build via `IntegrationEvent::new`. |
| `IntegrationCommand<T>` | Envelope for a request: `command_id`, `command_type`, `version: u8`, `issued_at`, `metadata`, `payload: T`. `#[non_exhaustive]` — build via `IntegrationCommand::new`. |
| `IntegrationError` | `Publish { kind, detail }` (transport), `Consume { kind, detail }` (bind/pull), `Decode { subject, detail }` (poison message), `Serialization(serde_json::Error)` (encoding). `#[non_exhaustive]`. |
| `PublishErrorKind` | Classifies a publish failure: `NoStream`, `Timeout`, `Other`. `#[non_exhaustive]`. |
| `ConsumeErrorKind` | Classifies a consume/bind failure: `NoStream`, `NoConsumer` (missing declared object at bind), `ConsumerGone` (the consumer vanished mid-run — deleted server-side or, for the awaiter, reaped past its `inactive_threshold`), `Other`. `#[non_exhaustive]`. |
| `IntegrationPublisher` (trait, object-safe) | `publish(subject, payload) -> Result<(), IntegrationError>` and fire-and-forget `publish_if_connected(subject, payload)`. |
| `IntegrationPublisherExt` (blanket trait) | Typed helpers: `publish_event`, `publish_command`, and `_if_connected` variants. |
| `NatsIntegrationPublisher` | JetStream-backed implementation, awaits the broker ack. |
| `NoopIntegrationPublisher` | No-op; for tests and as a default when messaging is disabled. |
| `DurableConsumer` / `Delivery` / `MessageOutcome` | The **receiver** consumer shape: binds a durable consumer and runs a typed handler over a parked message stream (see [Consuming](#consuming)). |
| `CorrelatedAwaiter` / `CorrelatedMatch` / `AwaiterConfig` | The **awaiter** consumer shape: an ephemeral consumer that resolves on a matching `correlation_id`; `AwaiterConfig` tunes its `inactive_threshold` (how long it stays armed across waits — default 300s). See [Consuming](#consuming). |
| `integration_subject` / `MessageKind` / `SubjectError` | Builds and validates the subject convention (see below). |
| `OutboxStatus` / `OutboxRecord` / `next_after_attempt` / `verify_consumer` | The **pure** outbox core (no feature, no `sqlx`): the staged-message value, the retry state machine, and the ungated receiver-online precheck (`async_nats` only). See [Transactional outbox](#transactional-outbox). |
| `OutboxStore` / `OutboxRelay` / `RelayPolicy` | The outbox's Postgres store + per-row-commit relay. Behind the **`outbox`** feature (pulls `sqlx`). |

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
    IntegrationPublisherExt, MessageKind, MessageMetadata, NatsIntegrationPublisher,
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

let metadata = MessageMetadata::new(
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
| **Awaiter** | `CorrelatedAwaiter` | You published a command and await its correlated reply (e.g. a declaring replica awaiting `…accepted` / `…rejected`). | A *per-boot ephemeral* consumer, created at runtime. | No — every replica sees all replies and filters its own by `correlation_id`. |

**No auto-provisioning.** Both bind by name and **fail loud** if the stream — or,
for `DurableConsumer`, the named consumer — is missing (`IntegrationError::Consume`
with `ConsumeErrorKind::NoStream` / `NoConsumer`). The lib never creates a stream
or a durable consumer. The awaiter *does* create its **ephemeral** consumer — a
read cursor, not infrastructure — but never the stream.

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

The safe protocol is **subscribe-first**: create the awaiter, *then* publish the
command, then await. On timeout, re-publish (same `correlation_id`) and await
again — the awaiter stays armed across waits **up to its configured
`inactive_threshold` of inactivity** (`AwaiterConfig`, default 300s), so no reply
is missed in between. A reply emitted *before* the awaiter exists is missed by
design; subscribe-first + re-publish makes that safe.

**Stays armed only up to `inactive_threshold`.** *During* a wait the pull stream
issues requests that keep the ephemeral consumer alive; *between* waits nothing
polls it. The server reaps an ephemeral consumer after `inactive_threshold` of
such inactivity, so beyond that bound the next `await_correlation` **fails loud**
with `ConsumeErrorKind::ConsumerGone` rather than silently missing the reply on a
recreated `New`-policy consumer. The default (300s) is generous; raise it with
`CorrelatedAwaiter::create_with(.., AwaiterConfig { inactive_threshold })` if the
gap between a timed-out wait and the next re-publish can exceed it.

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
// Need a longer armed window between waits? Use `create_with`
// (`AwaiterConfig` is non-exhaustive — start from `default()`):
// let mut config = AwaiterConfig::default();
// config.inactive_threshold = Duration::from_secs(600);
// CorrelatedAwaiter::create_with(&jetstream, "IDENTITY", subjects, config).await?;

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
   NOTHING`); the id is a creator-supplied UUIDv7.
2. **Relay** (post-commit, and the crash-recovery sweep): `OutboxRelay::run_once`
   processes `Pending` rows **one at a time, each in its own short transaction**
   (`BEGIN; SELECT … WHERE id > cursor FOR UPDATE SKIP LOCKED LIMIT 1; publish;
   apply_transition; COMMIT`), looping until none remain (or `max_messages`).
   `FOR UPDATE SKIP LOCKED` lets replicas drain disjoint rows. The *same* code
   that publishes right after a commit re-publishes a row a crash left `Pending`
   — there is no separate recovery path to forget.

   **Why per-row, not per-batch:** the publish IO is never held inside a
   transaction that locks a whole batch. A slow broker pins only the one row
   being published (and its connection), not 64 rows for the sum of 64 network
   round-trips; and a DB error on one row's transition rolls back *that row only*
   — never dozens of already-acked siblings. A per-pass `id` cursor makes each
   row attempted at most once per pass (a failed row retries on the next pass —
   the caller's interval is the backoff), so the pass never spins on a failing
   row.

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
canonical DDL the store binds to:

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

// 1) Stage in the SAME transaction as the domain write.
let subject = integration_subject("identity", MessageKind::Evt, "user", "created", 1)?;
let record = OutboxRecord::stage_event(Uuid::now_v7(), &subject, &event)?;
let mut tx = pool.begin().await?;
// … the domain write on `&mut *tx` …
stage(&mut *tx, &record).await?;
tx.commit().await?;

// 2) Run the relay on a schedule (and once at startup, for recovery).
let publisher: Arc<dyn IntegrationPublisher> = Arc::new(NatsIntegrationPublisher::new(jetstream));
let relay = OutboxRelay::new(pool.clone(), publisher);
let report = relay.run_once().await?; // { picked, published, failed, retried }
```

For a command whose receiver must be online before issuing it, `verify_consumer`
is an opt-in fail-fast precheck (`ConsumeErrorKind::NoConsumer` when no durable
consumer is bound) — separate from the relay, and it never auto-provisions.

## Install

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-integration = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-integration", tag = "br-core-integration-v0.4.0" }

# The transactional outbox (the `OutboxStore` + `OutboxRelay`) is behind an
# opt-in feature that pulls `sqlx`; the base crate stays DB-free without it:
# br-core-integration = { git = "...", package = "br-core-integration", tag = "br-core-integration-v0.4.0", features = ["outbox"] }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
