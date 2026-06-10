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
| `IntegrationError` | `Publish { kind: PublishErrorKind, detail: String }` for transport failures, `Serialization(serde_json::Error)` for encoding failures. `#[non_exhaustive]`. |
| `PublishErrorKind` | Classifies a publish failure: `NoStream`, `Timeout`, `Other`. `#[non_exhaustive]`. |
| `IntegrationPublisher` (trait, object-safe) | `publish(subject, payload) -> Result<(), IntegrationError>` and fire-and-forget `publish_if_connected(subject, payload)`. |
| `IntegrationPublisherExt` (blanket trait) | Typed helpers: `publish_event`, `publish_command`, and `_if_connected` variants. |
| `NatsIntegrationPublisher` | JetStream-backed implementation, awaits the broker ack. |
| `NoopIntegrationPublisher` | No-op; for tests and as a default when messaging is disabled. |
| `integration_subject` / `MessageKind` / `SubjectError` | Builds and validates the subject convention (see below). |

## Subject naming convention

Subjects follow `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}`
(e.g. `identity.evt.user.created.v1`, `notifier.cmd.notification.send.v1`).
Build them with `integration_subject` rather than formatting strings by hand —
it is the single source of the convention and validates that each segment is
non-empty and drawn from `[a-z0-9-]` (no `.`, no NATS wildcards, no whitespace):

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
    Actor::Human(UserId::from(Uuid::new_v4())),
    Uuid::new_v4(),
);

let evt = IntegrationEvent::new(
    Uuid::new_v4(),
    "user.created",
    1,
    Utc::now(),
    metadata,
    UserCreatedV1 {
        user_id: Uuid::new_v4(),
        email: "alice@example.com".into(),
    },
);

let subject = integration_subject("identity", MessageKind::Evt, "user", "created", 1)?;
publisher.publish_event(&subject, &evt).await?;
# Ok(()) }
```

For best-effort emission where the request must not fail because the bus is
down, use `publish_event_if_connected` / `publish_command_if_connected`.

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-integration = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-integration", tag = "br-core-integration-v0.2.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
