# br-core-integration

Transport-independent **integration contracts** — the typed shapes that travel
*between* bounded contexts on the message bus, with no transport coupling.

**Purpose.** Where [`br-core-events`](../br-core-events/) holds the shapes used
*inside* a context's event store, this crate holds the shapes used *between*
contexts: the message **coordinates** (`CommandCoords` / `EventCoords` and their
validated segments), the `IntegrationEvent<T>` / `IntegrationCommand<T>`
envelopes, the `MessageOutcome` a handler returns, and the **pure** outbox state
machine. It carries **no `async_nats`, no `sqlx`, no I/O** — the NATS transport
(publishers, durable consumers, the awaiter, the Postgres outbox store + relay)
lives in [`br-util-nats-fabric`](../br-util-nats-fabric/), which builds on these
contracts.

**When to use.** A service produces or consumes cross-context messages and needs
to agree with peers on the wire shape, the coordinates, and the metadata fields.

**When not to use.** In-context domain events — keep those in `br-core-events`.
Per-context concrete payload types (e.g. `UserCreatedV1 { … }`) — those belong to
the producing service. The actual NATS publish/consume/outbox machinery — that is
`br-util-nats-fabric`.

## What's inside

| Type | Role |
|---|---|
| `EventMetadata` | Re-export of `br_core_events::EventMetadata` — one type, one wire contract. Carries a typed `br_core_kernel::Actor` (human or machine), `correlation_id`, `causation_id` (skipped on the wire when `None`). Backward-compatible wire format: pre-`Actor` payloads (no `actor_kind`) default to a human actor. |
| `IntegrationEvent<T>` | Envelope for a fact: `event_id`, `event_type`, `version: u8`, `occurred_at`, `metadata`, `payload: T`. `#[non_exhaustive]` — build via `IntegrationEvent::new`. |
| `IntegrationCommand<T>` | Envelope for a request: `command_id`, `command_type`, `version: u8`, `issued_at`, `metadata`, `payload: T`. `#[non_exhaustive]` — build via `IntegrationCommand::new`. |
| `Bc` / `Aggregate` / `Verb` / `PastFact` / `CoordError` | Validated, transport-independent coordinate **segment** newtypes (`new` rejects empty / non-`[A-Za-z0-9_-]`; `as_str` / `AsRef` / `TryFrom<&str>`). Contract types with no NATS coupling. |
| `CommandCoords` / `EventCoords` | The typed integration coordinates — `CommandCoords { receiver, aggregate, verb, version }` / `EventCoords { producer, aggregate, fact, version }`. The NATS Fabric (`br-util-nats-fabric`) renders these onto the `integration.…` subject grammar; this crate holds only the validated data. |
| `MessageOutcome` | The verdict a consumer's handler returns for one message: `Ack` / `Nak(Option<Duration>)` / `Term`. `#[non_exhaustive]`. The transport (the Fabric) maps it onto the broker's ack semantics. |
| `OutboxStatus` / `Transition` / `next_after_attempt` / `UnknownOutboxStatus` | The **pure** outbox state machine (no feature, no `sqlx`, no `async_nats`): the persisted status with its total DB-string mapping (`as_db_str` / `from_db_str`), and `next_after_attempt(prior_attempts, max_attempts, succeeded) -> Transition` deciding the post-attempt status. The Fabric's outbox store drives this state machine. |
| `retry_backoff` / `RETRY_BACKOFF_BASE` / `RETRY_BACKOFF_MAX` | The exponential-backoff retry policy (base 500ms, doubling, capped at 30s, overflow-safe). Pure; the Fabric's relay calls it to schedule a transient-failure retry. |

## Message coordinates

Integration messages are addressed by **typed coordinates**, never by hand-built
subject strings. `CommandCoords { receiver, aggregate, verb, version }` names a
command addressed to a receiving context; `EventCoords { producer, aggregate,
fact, version }` names a fact a producer emits. Each segment is a validated
newtype (`Bc`, `Aggregate`, `Verb`, `PastFact`) that rejects empty or
out-of-charset input at construction, so an illegal coordinate is
unrepresentable. The `integration.…` subject grammar these render onto, and the
rendering itself, live in [`br-util-nats-fabric`](../br-util-nats-fabric/) — this
crate holds only the validated data.

```rust
use br_core_integration::{Aggregate, Bc, EventCoords, PastFact};

let coords = EventCoords {
    producer: Bc::new("identity").unwrap(),
    aggregate: Aggregate::new("user").unwrap(),
    fact: PastFact::new("created").unwrap(),
    version: 1,
};
assert_eq!(coords.producer.as_str(), "identity");
```

## Envelopes

```rust
use br_core_integration::{Actor, EventMetadata, IntegrationEvent, UserId};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct UserCreatedV1 { user_id: Uuid, email: String }

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
assert_eq!(evt.event_type, "user.created");
```

The envelope is serialized onto the bus by the producer and deserialized by the
consumer through these exact types — the Go wire anchor and the conformance
suites pin this shape, so a dropped or retyped field is a contract break, not a
local detail.

## Outbox state machine (pure)

The transactional outbox's **decision logic** is pure and lives here; its
Postgres store, the subscribe-driven relay, and the staged-record persistence
live in [`br-util-nats-fabric`](../br-util-nats-fabric/) (behind its `outbox`
feature). This crate owns:

- `OutboxStatus` — `Pending` / `Published` / `Failed`, with a total
  `as_db_str` / `from_db_str` mapping (`from_db_str` returns a typed
  `UnknownOutboxStatus` on an unknown value — no silent fallback) and
  `is_terminal`.
- `next_after_attempt(prior_attempts, max_attempts, succeeded) -> Transition` —
  the state transition after one publish attempt: success → `Published`; failure
  at the attempt cap → `Failed`; otherwise stays `Pending`, attempts
  incremented. `max_attempts` is clamped to at least 1.
- `retry_backoff(attempts)` — the exponential backoff (base
  `RETRY_BACKOFF_BASE` = 500ms, doubling, capped at `RETRY_BACKOFF_MAX` = 30s,
  overflow-safe) the relay uses to schedule a transient-failure retry.

```rust
use br_core_integration::{OutboxStatus, next_after_attempt};

let t = next_after_attempt(2, 3, false); // third failed attempt, cap 3
assert_eq!(t.status, OutboxStatus::Failed);
assert_eq!(t.attempts, 3);
```

## Install

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-integration = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-integration", tag = "v1.0.2", version = "1.0.2" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
