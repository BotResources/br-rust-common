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
| `MessageMetadata` | `actor_id`, `correlation_id`, `causation_id` (skipped on the wire when `None`). Kept separate from `EventMetadata` because it may diverge (e.g. `actor_id` becoming an `Actor` enum). |
| `IntegrationEvent<T>` | Envelope for a fact: `event_id`, `event_type`, `version: u8`, `occurred_at`, `metadata`, `payload: T`. |
| `IntegrationCommand<T>` | Envelope for a request: `command_id`, `command_type`, `version: u8`, `issued_at`, `metadata`, `payload: T`. |
| `IntegrationError` | `Publish(String)` for transport failures, `Serialization(serde_json::Error)` for encoding failures. |
| `IntegrationPublisher` (trait, object-safe) | `publish(subject, payload) -> Result<(), IntegrationError>` and fire-and-forget `publish_if_connected(subject, payload)`. |
| `IntegrationPublisherExt` (blanket trait) | Typed helpers: `publish_event`, `publish_command`, and `_if_connected` variants. |
| `NatsIntegrationPublisher` | JetStream-backed implementation, awaits the broker ack. |
| `NoopIntegrationPublisher` | No-op; for tests and as a default when messaging is disabled. |

## Subject naming convention

Not enforced by the type system, but recommended:

- Events:   `{bc}.evt.{aggregate}.{event_name}.v{N}`
  — e.g. `identity.evt.user.created.v1`
- Commands: `{bc}.cmd.{aggregate}.{command_name}.v{N}`
  — e.g. `notifier.cmd.notification.send.v1`

Subscribers use NATS wildcards (`identity.evt.>`, `notifier.cmd.>`) to consume
relevant streams.

## Usage

```rust
use std::sync::Arc;
use br_core_integration::{
    IntegrationEvent, IntegrationPublisher, IntegrationPublisherExt,
    MessageMetadata, NatsIntegrationPublisher,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize)]
struct UserCreatedV1 { user_id: Uuid, email: String }

# async fn example(jetstream: async_nats::jetstream::Context) -> Result<(), Box<dyn std::error::Error>> {
let publisher: Arc<dyn IntegrationPublisher> =
    Arc::new(NatsIntegrationPublisher::new(jetstream));

let evt = IntegrationEvent {
    event_id: Uuid::new_v4(),
    event_type: "user.created".into(),
    version: 1,
    occurred_at: Utc::now(),
    metadata: MessageMetadata {
        actor_id: Uuid::new_v4(),
        correlation_id: Uuid::new_v4(),
        causation_id: None,
    },
    payload: UserCreatedV1 {
        user_id: Uuid::new_v4(),
        email: "alice@example.com".into(),
    },
};

publisher
    .publish_event("identity.evt.user.created.v1", &evt)
    .await?;
# Ok(()) }
```

For best-effort emission where the request must not fail because the bus is
down, use `publish_event_if_connected` / `publish_command_if_connected`.

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-integration = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-integration", tag = "br-core-integration-v0.1.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
