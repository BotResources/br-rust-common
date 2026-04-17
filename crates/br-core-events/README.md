# br-core-events

Shared data structures for domain events.

**Purpose.** `EventMetadata`, `RawEvent`, and `DomainEvent` — the envelopes
used by every service that produces or persists domain events.

**When to use.** A service's event store, aggregate, or outbox needs to
share an event shape with other services (replay, analytics, projections).

**When not to use.** Integration events (cross-bounded-context payloads)
should be defined per-context, not here. This crate holds only the neutral
transport shapes.

**Current version.** `0.1.0`
