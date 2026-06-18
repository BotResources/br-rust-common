# br-util-broadcast

In-process **event bus** for **post-commit fan-out** of domain events to
same-process GraphQL subscriptions. A thin, domain-free wrapper over a
[`tokio::sync::broadcast`] channel. Tier `util` — types and functions, no
aggregate, no policy.

This is the in-process real-time fan-out from the backend doctrine (broadcast →
subscription resolvers). It is **not** the inter-service bus — that is NATS
JetStream / `br-core-integration`.

## The one contract: notify *after* commit

Domain events must be broadcast **after** the database transaction commits,
never inside it. Publishing inside the transaction is a real correctness bug
(be-botresources.ai#66): if the transaction later rolls back, subscribers have
already observed state that never persisted, and a client folding the event
stream diverges from the durable truth.

Rather than rely on a comment asking callers to "publish last", **the API shape
makes publishing before commit hard to write by accident** and the right order
self-documenting:

- **`PendingBroadcast<T>`** is the buffer a command fills while it runs. It
  **carries no channel** — there is no `send`, no reference to the bus, no way to
  reach a subscriber from it.
- **`EventBus<T>`** has **no method that takes a bare event**. The single publish
  path is **`EventBus::publish_after_commit`**, which *consumes* a
  `PendingBroadcast` and is named for the commit it must follow.

So the buffer (built during the command) and the channel are structurally
distinct. You carry the buffer to the one named fan-out method to emit — there is
no API to push a lone event mid-transaction.

**What this does *not* do:** the type system does not *prove* the transaction
committed before `publish_after_commit` runs — that ordering stays a caller
convention. Encoding it in types would mean threading a commit-witness token out
of `sqlx`, which this domain-free tier-`util` crate deliberately avoids. What the
crate removes is the trivial footgun the seed had — a bare `send(&event)`
callable from anywhere, including mid-transaction. The pipeline still owns the
ordering; the API makes the right order the obvious one. An end-to-end test on
the consumer side (rollback → no subscriber receives) is what actually closes
be-botresources.ai#66; it cannot live in this crate.

```rust,ignore
use br_util_broadcast::{EventBus, PendingBroadcast};

let bus: EventBus<DomainEvent> = EventBus::new(1024);

// a subscription resolver, elsewhere:
let mut rx = bus.subscribe();

// a command pipeline: load -> command (-> events) -> save -> dispatch
let events = aggregate.do_something()?;              // the domain decides
let pending = PendingBroadcast::from_events(events); // buffer, no fan-out yet
tx.commit().await?;                                  // durable truth lands first
let _ = bus.publish_after_commit(pending);           // ONLY now does it fan out
```

## Generic over the payload

The bus is generic over the event type `T: Clone` (typically
`br_core_events::DomainEvent`) so the mechanism stays domain-free. The crate
depends on no `br-core-*` / `br-util-*` crate.

## Best-effort by design

Fan-out is real-time notification, **not** a durable log. A lagged receiver (one
that fell more than `capacity` events behind) loses the oldest events and is told
so on its next `recv()`; a dropped receiver simply stops receiving. Either way
the client recovers by reconnect/replay against the **committed** state — never
data loss. `publish_after_commit` returns the single `BroadcastError`
(`NoSubscribers`, with the unheard count) as an **informational** signal a caller
may log/meter or ignore — it is never a persistence failure (the events are
already committed when it is called).

### Batch-publish semantics when subscribers disappear mid-batch

`publish_after_commit` sends a `PendingBroadcast`'s events **one at a time, in
order**, and stops at the **first** send that finds no live receiver. The
`unheard` count in `NoSubscribers` is the number of events **from the offending
one onward that were not sent** (`total - already-sent`), not a count of
receivers. So if the last receiver drops after the 2nd of 5 events lands,
`publish_after_commit` returns `NoSubscribers { unheard: 3 }`: the first two were
fanned out, the remaining three were not. The already-sent events are **not**
rolled back — fan-out is fire-and-forget per event. An empty buffer is a legal
no-op and returns `Ok`. Because all events are already committed before this is
called, a non-zero `unheard` never threatens durability; clients that missed it
recover by reconnect/replay.

### Capacity must be positive

`EventBus::new(capacity)` **panics** if `capacity == 0` — a zero-capacity
broadcast channel can buffer nothing and would drop every event before any
subscriber could see it. Capacity is a composition-root configuration value, so a
zero is a programming error, not a runtime condition; the panic carries a precise
message rather than failing opaquely deep inside `tokio`.

## Errors — codes, not language

`BroadcastError`'s `Display` is a **stable code** (`no_subscribers unheard=N`),
not UI prose; the human text and its i18n live at the edge. The enum is
`#[non_exhaustive]` (match with a wildcard).

## Tier & dependencies

Tier `util`: depends only on `tokio` (`sync` for the broadcast channel) and
`thiserror` (the error type). No I/O beyond the in-process channel, no `br-*`
dependency. Unified workspace versioning, distributed by git tag.

## Install

```toml
[dependencies]
br-util-broadcast = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-broadcast", tag = "v1.0.1" }
```

[`tokio::sync::broadcast`]: https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
