//! The post-commit buffer — events staged for fan-out, holding no channel.

/// Events staged for fan-out, **not yet broadcast**.
///
/// This is the load-bearing type of the crate. It is the *only* thing a command
/// builds while it runs, and it **carries no channel** — there is no `send`, no
/// reference to the [`EventBus`](crate::EventBus), no way for the buffered
/// events to reach a subscriber from here. The single path to fan-out is to
/// hand this buffer to [`EventBus::publish_after_commit`](crate::EventBus::publish_after_commit)
/// — a method whose name *is* the contract.
///
/// ## Why this shape (be-botresources.ai#66)
///
/// Publishing a domain event *inside* the database transaction is a correctness
/// bug: if the transaction then rolls back, subscribers have already seen state
/// that never persisted. The fix is **notify-after-commit** — and rather than
/// rely on a comment asking callers to "remember to publish last", this crate
/// makes the buffer (built during the command) structurally distinct from the
/// channel (reachable only after commit). You collect into a `PendingBroadcast`
/// freely; you cannot fan it out until you pass it across the commit boundary.
///
/// ## Shape of use
///
/// ```ignore
/// // 1. during the command — buffer freely, no fan-out possible yet
/// let mut pending = PendingBroadcast::new();
/// pending.extend(domain_events);
///
/// // 2. commit the transaction — the durable truth lands first
/// tx.commit().await?;
///
/// // 3. ONLY NOW can the events reach subscribers
/// bus.publish_after_commit(pending);
/// ```
///
/// `T` is the broadcast payload — typically `br_core_events::DomainEvent`, but
/// the buffer is generic so the mechanism stays domain-free (tier `util`).
#[derive(Debug, Clone)]
pub struct PendingBroadcast<T> {
    pub(crate) events: Vec<T>,
}

impl<T> PendingBroadcast<T> {
    /// An empty buffer.
    #[must_use]
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    /// A buffer seeded with `events` — e.g. the `Vec` a domain command returned.
    #[must_use]
    pub fn from_events(events: Vec<T>) -> Self {
        Self { events }
    }

    /// Stage one more event for the post-commit fan-out.
    pub fn push(&mut self, event: T) {
        self.events.push(event);
    }

    /// Stage every event from `events` for the post-commit fan-out.
    pub fn extend(&mut self, events: impl IntoIterator<Item = T>) {
        self.events.extend(events);
    }

    /// How many events are staged.
    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// Whether nothing is staged. A no-op fan-out is legal — a command that
    /// changed no state stages nothing and reaches no subscriber.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl<T> Default for PendingBroadcast<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FromIterator<T> for PendingBroadcast<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            events: iter.into_iter().collect(),
        }
    }
}
