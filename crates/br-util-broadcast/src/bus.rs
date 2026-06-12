//! The in-process bus handle: subscribe, and the after-commit fan-out path.

use tokio::sync::broadcast::{self, Receiver};

use crate::{BroadcastError, PendingBroadcast};

/// In-process event bus for fanning domain events out to same-process
/// subscribers (the GraphQL subscription resolvers).
///
/// Wraps a [`tokio::sync::broadcast`] channel; `clone` shares the same channel,
/// so the bus is injected once at the composition root and cloned wherever a
/// subscriber or a publisher needs it. `T` is the broadcast payload — typically
/// `br_core_events::DomainEvent`, kept generic so the crate stays domain-free
/// (tier `util`).
///
/// ## The one contract: notify *after* commit
///
/// The bus has **no method that takes a bare event** — you cannot fan an event
/// out by itself. The only publish entry point is
/// [`publish_after_commit`](Self::publish_after_commit), which consumes a
/// [`PendingBroadcast`] you built while the command ran and is named so the
/// ordering is self-documenting. This is deliberate: publishing inside the DB
/// transaction is the be-botresources.ai#66 bug (a later rollback leaves
/// subscribers having seen state that never persisted). The buffer-vs-channel
/// split makes the wrong order — *notify before commit* — **hard to write by
/// accident**, because the buffer carries no channel and the one fan-out method
/// names the commit. It does **not** prove the commit happened first: that stays
/// a caller convention the type system cannot verify without coupling this util
/// crate to `sqlx`. The pipeline still owns the ordering; the API makes the
/// right order the obvious one.
///
/// ```ignore
/// let bus: EventBus<DomainEvent> = EventBus::new(1024);
///
/// // a subscription resolver, elsewhere:
/// let mut rx = bus.subscribe();
///
/// // a command pipeline:
/// let pending = PendingBroadcast::from_events(events); // built during the command
/// tx.commit().await?;                                  // durable truth lands first
/// bus.publish_after_commit(pending);                   // ONLY now does it fan out
/// ```
///
/// Fan-out is **best-effort by design** (real-time notification, not a durable
/// log): a lagged or dropped receiver only costs that client a reconnect/replay
/// against the committed state — never data. See [`BroadcastError`].
#[derive(Clone, Debug)]
pub struct EventBus<T> {
    sender: broadcast::Sender<T>,
}

impl<T: Clone> EventBus<T> {
    /// A new bus whose channel buffers up to `capacity` events per receiver.
    ///
    /// A receiver that falls more than `capacity` events behind is **lagged**:
    /// it loses the oldest events and its next `recv()` yields
    /// [`RecvError::Lagged`](broadcast::error::RecvError::Lagged). Size
    /// `capacity` for the burst a single client can fall behind during a slow
    /// poll; a lagged client recovers by reconnect/replay against the committed
    /// state.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    /// Subscribe to every event fanned out **after this call**.
    ///
    /// Events published before the subscription are not delivered (a subscriber
    /// catches up by replaying committed state, not via the bus). Each call
    /// returns an independent receiver; dropping it unsubscribes.
    #[must_use]
    pub fn subscribe(&self) -> Receiver<T> {
        self.sender.subscribe()
    }

    /// How many receivers are currently subscribed.
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    /// Fan a [`PendingBroadcast`] out to all current subscribers — the **only**
    /// publish path, to be called **after the transaction has committed**.
    ///
    /// Consumes the buffer and broadcasts each staged event in order. Returns
    /// `Ok(())` when at least one subscriber was listening (or the buffer was
    /// empty — a no-op fan-out is legal); returns
    /// [`BroadcastError::NoSubscribers`] when the channel was found empty before
    /// every staged event had been broadcast, carrying the count of events
    /// **not yet broadcast** at that point. That signal is informational,
    /// **not** a write failure: the events are already committed and durable, so
    /// a caller may log/meter it or ignore it
    /// (`let _ = bus.publish_after_commit(pending);`).
    ///
    /// Naming the method and consuming a buffer that carries no channel of its
    /// own is what makes publish-before-commit *hard to write by accident and
    /// self-documenting*. It does **not** verify that the transaction committed
    /// first: that ordering is a caller convention the type system does not
    /// prove (the crate stays domain-free, with no `sqlx` dependency). There is
    /// simply no API to push a lone event mid-transaction.
    pub fn publish_after_commit(&self, pending: PendingBroadcast<T>) -> Result<(), BroadcastError> {
        let total = pending.events.len();
        for (sent, event) in pending.events.into_iter().enumerate() {
            // `send` errors only when there are zero receivers; the events are
            // already durable, so a no-listener fan-out is a benign signal, not
            // a failure. `unheard` is the tail that had not yet been broadcast
            // when the channel was found empty (best-effort, accurate even if
            // receivers drop mid-fan-out).
            if self.sender.send(event).is_err() {
                return Err(BroadcastError::NoSubscribers {
                    unheard: total - sent,
                });
            }
        }
        Ok(())
    }
}
