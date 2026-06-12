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
makes the correct order the only order it can express**:

- **`PendingBroadcast<T>`** is the buffer a command fills while it runs. It
  **carries no channel** — there is no `send`, no reference to the bus, no way to
  reach a subscriber from it.
- **`EventBus<T>`** has **no method that takes a bare event**. The single publish
  path is **`EventBus::publish_after_commit`**, which *consumes* a
  `PendingBroadcast` and is named so the ordering is impossible to miss.

So the buffer (built during the command) and the channel (reachable only after
commit) are structurally distinct. You must carry the buffer across the commit
boundary to fan it out — there is no API to push a lone event mid-transaction.

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

## Errors — codes, not language

`BroadcastError`'s `Display` is a **stable code** (`no_subscribers unheard=N`),
not UI prose; the human text and its i18n live at the edge. The enum is
`#[non_exhaustive]` (match with a wildcard).

## Tier & dependencies

Tier `util`: depends only on `tokio` (`sync` for the broadcast channel) and
`thiserror` (the error type). No I/O beyond the in-process channel, no `br-*`
dependency. Per-crate semver, distributed by git tag.

## Install

```toml
[dependencies]
br-util-broadcast = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-broadcast", tag = "br-util-broadcast-v0.1.0" }
```

[`tokio::sync::broadcast`]: https://docs.rs/tokio/latest/tokio/sync/broadcast/index.html

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
