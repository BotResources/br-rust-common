# br-core-events

Shared envelope types for domain events: what the event store persists and the
metadata that travels alongside.

**Purpose.** Two plain data structures used by every service that produces
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
| `EventMetadata` | Identity + correlation context attached to each event. Fields: `actor: br_core_kernel::Actor` (human or machine), `correlation_id: Uuid`, `causation_id: Option<Uuid>` (skipped when `None` on the wire). |
| `DomainEvent` | What the event store stores and replays. Fields: `id`, `aggregate_id`, `aggregate_type`, `event_type`, `payload: serde_json::Value`, `metadata: EventMetadata`, `occurred_at: DateTime<Utc>`. |

`EventMetadata` and `DomainEvent` are `Serialize + Deserialize`. `DomainEvent`
carries a **typed** `EventMetadata` — not a loose `serde_json::Value` — so an
event can never persist a malformed metadata bag.

Both are `#[non_exhaustive]`: construct them through their constructors
(`EventMetadata::new` / `with_causation`, `DomainEvent::new`), not struct
literals. Fields stay `pub` for read access.

### Actor & wire compatibility

`EventMetadata` carries a typed `Actor` (human or machine) rather than a bare
`actor_id: Uuid`. The JSON **wire format is backward-compatible**: it serializes
a flat `actor_id` + `actor_kind` (`"human"` | `"service"`), and a payload
written before this field existed (no `actor_kind`) deserializes to a human
actor. An unknown `actor_kind` value is a hard error — it fails closed, never
defaults.

## Usage

```rust
use br_core_events::{DomainEvent, EventMetadata};
use br_core_events::{Actor, UserId};
use chrono::Utc;
use uuid::Uuid;

// Outbox / event store wraps the aggregate's fact with identity + persistence.
let meta = EventMetadata::new(Actor::Human(UserId::from(user_id)), req_id);

let event = DomainEvent::new(
    Uuid::new_v4(),
    order_id,
    "Order",
    "OrderPlaced",
    serde_json::json!({ "amount_cents": 1999 }),
    meta,
    Utc::now(),
);
```

Add to `Cargo.toml`:

```toml
[dependencies]
br-core-events = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-events", tag = "v1.0.1" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
