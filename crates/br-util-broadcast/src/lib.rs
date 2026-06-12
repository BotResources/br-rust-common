//! In-process event bus for **post-commit fan-out** of domain events to
//! same-process GraphQL subscriptions.
//!
//! A thin, domain-free wrapper over a [`tokio::sync::broadcast`] channel (tier
//! `util`: types and functions, no aggregate, no policy). It is the in-process
//! real-time fan-out described in the backend doctrine — *not* the inter-service
//! bus (that is NATS / `br-core-integration`).
//!
//! ## The one contract this crate exists to enforce: notify *after* commit
//!
//! Domain events must be broadcast **after** the database transaction commits,
//! never inside it. Publishing inside the transaction is a real correctness bug
//! (be-botresources.ai#66): if the transaction later rolls back, subscribers
//! have already observed state that never persisted, and a client folding the
//! event stream diverges from the durable truth.
//!
//! Rather than rely on a comment asking callers to "publish last", the API shape
//! makes publishing before commit **hard to write by accident** and the right
//! order self-documenting:
//!
//! - [`PendingBroadcast`] is the buffer a command fills while it runs. It
//!   **carries no channel** — there is no way to reach a subscriber from it.
//! - [`EventBus`] has **no method that takes a bare event**. The single publish
//!   path is [`EventBus::publish_after_commit`], which consumes a
//!   `PendingBroadcast` and is named for the commit it must follow.
//!
//! So the buffer (built during the command) and the channel are structurally
//! distinct; you must carry the buffer to the one named fan-out method to emit.
//! The type system does **not** prove the commit ran first — that stays a caller
//! convention (the crate stays domain-free, no `sqlx` dependency); what the API
//! removes is the trivial footgun of a bare `send` callable mid-transaction.
//!
//! ```ignore
//! use br_util_broadcast::{EventBus, PendingBroadcast};
//!
//! let bus: EventBus<DomainEvent> = EventBus::new(1024);
//!
//! // a subscription resolver subscribes:
//! let mut rx = bus.subscribe();
//!
//! // a command pipeline: load -> command (-> events) -> save -> dispatch
//! let events = aggregate.do_something()?;              // domain decides
//! let pending = PendingBroadcast::from_events(events); // buffer, no fan-out yet
//! tx.commit().await?;                                  // durable truth lands first
//! let _ = bus.publish_after_commit(pending);           // ONLY now does it fan out
//! ```
//!
//! ## Best-effort by design
//!
//! Fan-out is real-time notification, not a durable log: a lagged or dropped
//! receiver only costs that client a reconnect/replay against the committed
//! state, never data. The single [`BroadcastError`] variant
//! ([`NoSubscribers`](BroadcastError::NoSubscribers)) is an informational signal
//! a caller may log or ignore — it is never a persistence failure.

mod bus;
mod error;
mod pending;

pub use bus::EventBus;
pub use error::BroadcastError;
pub use pending::PendingBroadcast;
