# Changelog — br-core-integration

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.4.0] — 2026-06-12

Additive minor bump: the crate gains the **transactional outbox** — stage a
critical integration message in the *same transaction* as the domain write it
announces, then publish it post-commit with a relay that doubles as the
crash-recovery sweep. No existing public surface changes. The Postgres-touching
half is behind a new opt-in `outbox` feature, so the base crate stays free of a
`sqlx` dependency.

**Added**
- `outbox` module — the transactional outbox. Closes the window where a critical
  integration event is lost between the domain commit and the bus publish (a
  crash, a broker blip): a lost event in a choreography is the hardest bug to
  diagnose because nothing errors — the producer succeeded, the consumer simply
  never heard.
- **Pure state machine (always available, no feature):**
  - `OutboxStatus` (`#[non_exhaustive]`): `Pending` → `Published` (terminal) or
    `Failed` (terminal). Total `&str` ↔ enum mapping (`as_db_str` / `from_db_str`)
    — an unknown stored value is a typed `UnknownOutboxStatus`, never coerced
    into a default.
  - `Transition` + `next_after_attempt(prior_attempts, max_attempts, succeeded)`
    — the retry policy as a pure function (success → `Published`; failure at the
    cap → `Failed`; failure with retries left → stays `Pending`). Unit-tested as
    a spec.
  - `OutboxRecord` + `OutboxRecord::stage` / `stage_event` / `stage_command` —
    the staged message as a typed value; the id is a creator-supplied **UUIDv7**
    so a re-stage after a retried request is idempotent.
- **`outbox` feature (pulls `sqlx`, with `sqlx/uuid` + `sqlx/json`):**
  - `stage` / `stage_into` — the same-transaction insert. Takes the caller's
    executor (`&mut *tx`) so the row lands atomically with the domain write;
    `ON CONFLICT (id) DO NOTHING` keeps a retried stage idempotent.
  - `OutboxStore` — the relay's read/transition queries. `fetch_pending` uses
    `FOR UPDATE SKIP LOCKED` so concurrent relay replicas drain disjoint
    batches; it re-validates each row's status on hydration (an unknown value is
    a fail-loud `OutboxStoreError::UnknownStatus`).
  - `OutboxRelay` + `RelayPolicy` + `RelayReport` — the post-commit /
    crash-recovery publisher. `run_once` picks a locked batch, publishes each
    through the shared `IntegrationPublisher`, and persists the transition the
    pure state machine decides — all in one transaction so the lock is held
    until the transition is durable. Idle (no spin) when the outbox is empty;
    the caller owns the schedule and runs one pass at startup as the recovery
    sweep.
  - `verify_consumer(jetstream, stream, consumer)` — the honest, opt-in form of
    the medisup seed's `check_consumer`: a fail-fast (`ConsumeErrorKind::NoConsumer`)
    for a command whose receiver must be online. Separate from the relay; never
    auto-provisions.

**Semantics (documented honestly)**
- **At-least-once, post-commit.** Publish happens after the staging transaction
  commits (no dirty-read of an uncommitted producer write). A crash after the
  broker ack but before the status update leaves the row `Pending`, and the next
  relay pass re-publishes it — subscribers must de-dupe on the envelope id (the
  same idempotency rule the consumer shapes already document). No exactly-once.
- **The table is a declared object.** The store assumes `integration_outbox`
  (or a service-named table) already exists — the consuming service's migrations
  own it (canonical DDL in the README and the `outbox` module docs). A missing
  table is a fail-loud sqlx error; the lib never runs `CREATE TABLE`.

**Notes**
- The base crate (publishers, the durable consumer, the awaiter, and the pure
  outbox state machine) carries **no** `sqlx` dependency — only enabling the
  `outbox` feature links it.
- The `outbox` e2e (`tests/outbox_e2e.rs`) runs against **real** Postgres + NATS
  (no infra mocks), compiled only with `--features outbox` and `#[ignore]` by
  default — opt in with `DATABASE_URL` + `NATS_URL` set and
  `cargo test -p br-core-integration --features outbox --test outbox_e2e -- --ignored`.

## [0.3.1] — 2026-06-10

**Docs**
- The `README.md` is now the crate's root doc (`#![doc = include_str!(..)]`), so
  its `rust` usage examples (publishing, the durable receiver, the correlated
  awaiter) are compiled and run as doctests by `cargo test` — the README can no
  longer drift from the code. The `lib.rs` module docs keep only the rustdoc
  cross-links the README leaves to the reference, no longer a hand-synced
  duplicate of the README prose.
- Fixed the publishing example so it compiles: it built ids with
  `Uuid::new_v4()`, but the `uuid` dependency enables only `v5` / `v7` (not
  `v4`), so that call resolved to nothing. It now uses `Uuid::now_v7()`, the
  creator-generated UUIDv7 the codebase uses for ids. The example never compiled
  before, so the broken call shipped unnoticed — exactly the drift this change
  closes mechanically.

## [0.3.0] — 2026-06-10

Additive minor bump: the crate gains the **consuming** side in two deliberately
different shapes (a durable receiver and an ephemeral correlated awaiter). No
existing public surface changes; `IntegrationError` (already
`#[non_exhaustive]`) gains two variants. Match it with a wildcard arm.

**Added**
- `DurableConsumer` — the **receiver** shape. Binds a *pre-declared* durable
  consumer by name on a *pre-declared* stream (`DurableConsumer::bind`) and runs
  a typed handler over `consumer.messages()`, which **parks at zero CPU** — never
  a `fetch()` loop (proven by an idle-CPU e2e test). `run_commands` /
  `run_events` decode each message into `IntegrationCommand<T>` /
  `IntegrationEvent<T>`; the handler returns an explicit `MessageOutcome`
  (`Ack` / `Nak(delay)` / `Term`). Delivery is **at-least-once** with explicit
  ack (no exactly-once); a handler that needs once-only must be idempotent.
  Multiple workers binding the **same durable name** *share* delivery (JetStream
  pull work-sharing — documented honestly as *not* a core-NATS queue group).
- `Delivery<E>` — a decoded delivery handed to the handler (`subject` +
  `envelope`); `MessageOutcome` (`#[non_exhaustive]`) — the handler's ack
  decision, mapped to the JetStream ack wire.
- Poison-message handling: a payload that fails to deserialize into the typed
  envelope is **termed** (so it is not redelivered forever) and surfaced through
  the `on_poison` hook as `IntegrationError::Decode` — fail-closed, never a
  silent drop, never an infinite redelivery loop. Documented as a deliberate
  choice.
- `CorrelatedAwaiter` — the **awaiter** shape. A per-replica, per-boot
  *ephemeral* consumer (`CorrelatedAwaiter::create` / `::create_with`) over one or
  more filter subjects on a *pre-declared* stream that resolves when a delivered
  message's `metadata.correlation_id` matches the awaited value, ignoring
  everything else. `await_correlation(correlation_id, deadline)` returns
  `Ok(Some(match))` on a correlated match, `Ok(None)` on deadline (the awaiter
  stays **armed** — it can be re-awaited after a re-publish with no gap, **up to
  the configured `inactive_threshold` of inactivity**; beyond that the server
  reaps the ephemeral consumer and the next wait fails loud with
  `ConsumeErrorKind::ConsumerGone`). `CorrelatedMatch` reports the matched
  `subject` (so the caller picks the right payload type across e.g.
  `accepted` / `rejected`), the `metadata`, and the raw `payload` bytes. Deliver
  policy is `New`: confirmations emitted before the awaiter exists are missed by
  design — the subscribe-first + re-publish-on-timeout protocol makes that safe
  (documented).
- `AwaiterConfig` (`#[non_exhaustive]`) — tunes the awaiter's ephemeral consumer.
  `inactive_threshold` (default `AwaiterConfig::DEFAULT_INACTIVE_THRESHOLD`, 300s)
  is set **explicitly** at creation: leaving it `Duration::ZERO` (the
  `..Default::default()` value) makes serde skip it, so the broker applies its own
  short ephemeral default (~5s) and reaps the consumer between waits — the
  no-missed-reply property would then hold only briefly. The explicit threshold
  keeps the awaiter armed across the re-publish gap; `create_with` overrides it.
- `ConsumeErrorKind` enum (`#[non_exhaustive]`): `NoStream`, `NoConsumer`,
  `ConsumerGone`, `Other`. `IntegrationError::Consume { kind, detail }` and
  `IntegrationError::Decode { subject, detail }` variants. Both consumer shapes
  **fail loud** on a missing declared object — the lib never auto-provisions a
  stream or a durable consumer; the awaiter may create its *ephemeral* consumer
  (a read cursor, not infrastructure) but never the stream. A consumer that
  vanishes *mid-run* (deleted server-side, or — for the awaiter — reaped past its
  `inactive_threshold`) surfaces as `ConsumerGone`, classified honestly from
  async-nats' `MessagesErrorKind` (`ConsumerDeleted` / `MissingHeartbeat`); the
  underlying error text is preserved in `detail`, never discarded behind a fixed
  string.

**Changed**
- `integration_subject` segment charset widened from `[a-z0-9-]` to
  `[a-z0-9_-]`: multi-word segments are snake_case in the shared contract
  (e.g. `identity.cmd.service_scope.declare.v1`), and the builder must be able
  to produce them. Purely accepting — no previously valid input changes.
- `futures-util` and `tokio` moved from `dev-dependencies` to `dependencies`
  (the consumer message stream and the awaiter's per-wait deadline use them).

**Notes**
- **No graceful drain on `DurableConsumer::run_*` (API limitation).** Stopping a
  consumer means aborting its task; a message in flight at abort is neither acked
  nor naked, so it is redelivered after `AckWait` (at-least-once covers
  correctness — expect redelivery latency on rollouts). A `CancellationToken`
  drain is a planned additive addition.
- `MessageOutcome::Nak(None)` redelivers at the consumer's server-configured
  `AckWait` (not immediately); repeated naks redeliver at that cadence. Prefer
  `Nak(Some(delay))` for explicit backoff.
- Migrating `svc-notifier`'s hand-rolled durable consumer onto `DurableConsumer`
  is future work, not part of this release.

## [0.2.0] — 2026-06-10

Breaks the **Rust API**; the **JSON wire format stays backward-compatible** —
the metadata change is inherited from `br-core-events` 0.4.0, which keeps
pre-`Actor` payloads (no `actor_kind`) deserializing (defaulting to a human
actor). Internal module layout was split, but **all public import paths are
unchanged** (everything is still importable from the crate root).

**Changed (BREAKING)**
- `MessageMetadata` is now a re-export of `br_core_events::EventMetadata`
  (`pub use … as MessageMetadata`), not a hand-synced local duplicate. The
  public name is unchanged, but the type now carries a typed
  `br_core_kernel::Actor` instead of `actor_id: Uuid`, and a new dependency on
  `br-core-events`. It inherits that crate's wire contract and its
  legacy-defaults-to-human guarantee. *Migration:* `metadata.actor_id` →
  `metadata.actor.id()`; construct via `MessageMetadata::new(actor, correlation_id)`.
- `IntegrationError::Publish(String)` → `Publish { kind: PublishErrorKind, detail: String }`.
  `kind` classifies the failure (`NoStream`, `Timeout`, `Other`) so callers can
  branch without parsing a string. `NoStream` is the production-meaningful
  case: a publish to a subject no stream captures (a missing declared stream).
- `IntegrationEvent<T>`, `IntegrationCommand<T>`, and `IntegrationError` are now
  `#[non_exhaustive]`. Struct-literal construction of the envelopes from outside
  the crate is no longer possible; use the constructors. Match `IntegrationError`
  and `PublishErrorKind` with a wildcard arm.

**Added**
- `PublishErrorKind` enum (`#[non_exhaustive]`): `NoStream`, `Timeout`, `Other`,
  classified honestly from async-nats 0.48's own `PublishErrorKind`. Ambiguous
  kinds — including `BrokenPipe` — map to `Other` rather than claiming a cause
  the transport did not report.
- Constructors: `IntegrationEvent::new(...)`, `IntegrationCommand::new(...)`.
- `integration_subject(bc, kind, aggregate, name, version)` + `MessageKind`
  (`Cmd`/`Evt`) + `SubjectError` — builds `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}`
  and validates each segment is non-empty and drawn from `[a-z0-9-]` (no `.`,
  no NATS wildcards `*`/`>`, no whitespace — a malformed segment can neither
  break the subject structure nor collide with subscriber wildcards). The
  subject convention now lives in this helper instead of being repeated as prose.
- `Actor`, `UserId`, `ServiceAccountId` re-exported at the crate root (via
  `br-core-events`) so consumers can construct `MessageMetadata` without adding
  a direct `br-core-kernel` dependency.

**Docs**
- Documented why the envelopes' `version` field is `u8`: a published-contract
  schema version is bumped only on a breaking payload change and never
  realistically exceeds 255, so a wider integer would be dead range on the
  wire. No code change.
- E2E `tests/nats.rs` teardown cleanup: removed a redundant `delete_message(1)`
  no-op that ran just before deleting the whole stream, and teardown failures
  now `eprintln!` instead of being fully silenced — a leaked stream can capture
  a later test's messages, so a cleanup failure should be diagnosable.

**Migration**

Struct literals → constructors, and the new `Publish` shape:

```rust
// before (0.1.0)
let evt = IntegrationEvent {
    event_id, event_type: "user.created".into(), version: 1,
    occurred_at, metadata, payload,
};
let meta = MessageMetadata { actor_id, correlation_id, causation_id: None };
match err { IntegrationError::Publish(s) => /* string */ }

// after (0.2.0)
use br_core_integration::{Actor, UserId};
let evt = IntegrationEvent::new(event_id, "user.created", 1, occurred_at, metadata, payload);
let meta = MessageMetadata::new(Actor::Human(UserId::from(actor_id)), correlation_id);
match err { IntegrationError::Publish { kind, detail } => /* branch on kind */, _ => {} }
```

Reading the actor's uuid: `metadata.actor_id` → `metadata.actor.id()`.

## [0.1.0] — 2026-05-22

Initial release. Provides:

- `MessageMetadata`, `IntegrationEvent<T>`, `IntegrationCommand<T>` envelopes.
- `IntegrationError` (publish / serialization).
- Object-safe `IntegrationPublisher` trait with `publish` and
  `publish_if_connected` methods.
- `IntegrationPublisherExt` blanket helpers for typed publishing
  (`publish_event`, `publish_command`, `_if_connected` variants).
- `NatsIntegrationPublisher` (JetStream, awaits broker ack on `publish`,
  logs and swallows errors on `publish_if_connected`).
- `NoopIntegrationPublisher` for tests.
