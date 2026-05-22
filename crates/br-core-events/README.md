# br-core-events

Shared envelope types for domain events: what aggregates emit, what the
event store persists, and the metadata that travels alongside.

**Purpose.** Three plain data structures used by every service that produces
or persists domain events. Keeping them in one crate guarantees that
replays, analytics jobs, and projections agree on the wire shape.

**When to use.** A service's event store, aggregate, or outbox needs to
share an event shape with other services (replay, analytics, projections).

**When not to use.** Integration events (cross-bounded-context payloads)
should be defined per-context, not here. This crate holds only the neutral
transport shapes.

## What's inside

| Type | Role |
|---|---|
| `EventMetadata` | Identity + correlation context attached to each event. Fields: `actor_id: Uuid`, `correlation_id: Uuid`, `causation_id: Option<Uuid>` (skipped when `None` on the wire). |
| `RawEvent` | What an aggregate emits **before** persistence — no ID, no timestamp, no metadata yet. Fields: `aggregate_type: String`, `aggregate_id: Uuid`, `event_type: String`, `payload: serde_json::Value`. |
| `DomainEvent` | What the event store stores and replays. Fields: `id`, `aggregate_id`, `aggregate_type`, `event_type`, `payload`, `metadata: serde_json::Value`, `occurred_at: DateTime<Utc>`. |

`EventMetadata` and `DomainEvent` are `Serialize + Deserialize`. `RawEvent`
is not — it's an in-process producer-side type.

## Usage

```rust
use br_core_events::{DomainEvent, EventMetadata, RawEvent};
use chrono::Utc;
use uuid::Uuid;

// Aggregate emits a raw event.
let raw = RawEvent {
    aggregate_type: "Order".into(),
    aggregate_id: order_id,
    event_type: "OrderPlaced".into(),
    payload: serde_json::json!({ "amount_cents": 1999 }),
};

// Outbox / event store wraps it with identity + persistence fields.
let meta = EventMetadata {
    actor_id: user_id,
    correlation_id: req_id,
    causation_id: None,
};

let event = DomainEvent {
    id: Uuid::new_v4(),
    aggregate_id: raw.aggregate_id,
    aggregate_type: raw.aggregate_type,
    event_type: raw.event_type,
    payload: raw.payload,
    metadata: serde_json::to_value(&meta).unwrap(),
    occurred_at: Utc::now(),
};
```

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-events = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-events", tag = "br-core-events-v0.3.1" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
