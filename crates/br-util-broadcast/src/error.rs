//! The crate's own error type — codes, not language.

use thiserror::Error;

/// Outcome of a fan-out attempt.
///
/// Publishing to an in-process broadcast is **best-effort by design**: the bus
/// is a real-time notification channel, not a durable log. The durable truth is
/// the committed database row (the events were already persisted *before* the
/// fan-out — that is the whole contract; see [`EventBus`](crate::EventBus)). A
/// dropped notification only costs a client a reconnect/replay, never data.
///
/// So the only meaningful "error" is the informational signal **no subscriber
/// was listening** — the events still committed, nobody was on the channel to
/// hear them. It is returned (rather than swallowed) so a caller that *wants* to
/// log or meter "broadcast reached zero subscribers" can, without it ever being
/// mistaken for a persistence failure.
///
/// Per the codes-not-language rule the `#[error(...)]` string is a **stable
/// code**, never UI prose; human text and i18n live at the edge.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum BroadcastError {
    /// The fan-out found **no receiver subscribed** — nobody was on the channel
    /// to hear the events. `unheard` carries the count of events not yet
    /// broadcast when the channel was found empty.
    ///
    /// Not a failure of the write: the events are already committed and
    /// durable. Stable code: `no_subscribers`.
    #[error("no_subscribers unheard={unheard}")]
    NoSubscribers {
        /// Number of events that found no listener.
        unheard: usize,
    },
}
