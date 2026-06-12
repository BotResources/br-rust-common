//! The relay's health signal — a `watch` channel the [`run`](super::OutboxRelay::run)
//! loop updates and a consuming service bridges into its readiness gate.
//!
//! ## Why a `watch`, and why it lives here (not in `br-util-axum-readiness`)
//!
//! A structural publish failure (the target JetStream stream is not declared)
//! is an infra-declaration fault the relay cannot work around — it must be
//! *surfaced*, not silently retried. The relay publishes its state on a
//! [`tokio::sync::watch`] channel ([`RelayHealth`]); the latest value is always
//! readable without blocking, and a service can `await` a change.
//!
//! `br-core-integration` is a **`core`** crate and must not depend on a `util`
//! crate, so it cannot reach into `br-util-axum-readiness` to flip a readiness
//! flag itself (the tier rule: `core` never depends on `util`). Instead it
//! exposes the raw signal and the **consuming service** does the bridge — read
//! the watch, map `Degraded` to a 503 on the readiness endpoint. The README
//! shows that wiring as a seam; the lib provides the mechanism, the service the
//! policy.

use tokio::sync::watch;

/// The relay's health, published on a [`watch`] channel.
///
/// `#[non_exhaustive]`: a future state is additive; match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelayHealth {
    /// The relay is draining normally (the last pass hit no structural fault).
    Healthy,
    /// A structural publish failure was seen — the target JetStream stream is
    /// not declared. The affected row stays `Pending` (its attempt budget is
    /// *not* burned) and the relay re-drains on the next wake; `reason` is a
    /// stable, language-free code the consuming service surfaces. A later
    /// structural-free pass returns the relay to [`Healthy`](Self::Healthy).
    Degraded {
        /// A stable reason **code** (not a human sentence — codes-not-language):
        /// e.g. [`REASON_NO_STREAM`].
        reason: &'static str,
    },
}

/// The `Degraded` reason code raised when a publish fails with `NoStream`: the
/// target JetStream stream is not declared. Stable and language-free — the
/// consuming service maps it to operator-facing text at its edge.
pub const REASON_NO_STREAM: &str = "outbox.publish.no_stream";

/// A read handle on the relay's health, handed to the consuming service so it can
/// bridge the signal into its readiness gate. Cloneable; the latest value is
/// always readable via [`watch::Receiver::borrow`] without blocking.
pub type RelayHealthReceiver = watch::Receiver<RelayHealth>;

/// The relay's owned health publisher: the [`watch::Sender`] plus its initial
/// [`RelayHealthReceiver`], created at [`Healthy`](RelayHealth::Healthy).
///
/// The relay keeps the sender and updates it; [`receiver`](Self::receiver) hands
/// a read handle to the service. `set` only sends when the state actually
/// changes, so a steady stream of `Healthy` passes does not wake watchers.
pub(crate) struct RelayHealthChannel {
    sender: watch::Sender<RelayHealth>,
    receiver: RelayHealthReceiver,
}

impl RelayHealthChannel {
    /// A fresh channel seeded `Healthy`.
    pub(crate) fn new() -> Self {
        let (sender, receiver) = watch::channel(RelayHealth::Healthy);
        Self { sender, receiver }
    }

    /// A read handle for the consuming service's readiness bridge.
    pub(crate) fn receiver(&self) -> RelayHealthReceiver {
        self.receiver.clone()
    }

    /// Publish `health`, but only if it differs from the current value — a
    /// no-op transition does not wake watchers. `send_if_modified` keeps the
    /// channel alive even when every receiver has dropped (a relay with no
    /// readiness bridge is legitimate).
    pub(crate) fn set(&self, health: RelayHealth) {
        self.sender.send_if_modified(|current| {
            if *current == health {
                false
            } else {
                *current = health;
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GIVEN a fresh channel WHEN inspected THEN it starts Healthy
    #[test]
    fn starts_healthy() {
        let ch = RelayHealthChannel::new();
        assert_eq!(*ch.receiver().borrow(), RelayHealth::Healthy);
    }

    // GIVEN a degraded transition WHEN published THEN the receiver observes it
    #[test]
    fn publishes_a_degraded_transition() {
        let ch = RelayHealthChannel::new();
        let rx = ch.receiver();
        ch.set(RelayHealth::Degraded {
            reason: REASON_NO_STREAM,
        });
        assert_eq!(
            *rx.borrow(),
            RelayHealth::Degraded {
                reason: REASON_NO_STREAM
            }
        );
    }

    // GIVEN a repeated identical state WHEN published twice THEN no change is
    // signalled the second time (a steady state does not wake watchers).
    #[test]
    fn identical_state_does_not_mark_changed() {
        let ch = RelayHealthChannel::new();
        let mut rx = ch.receiver();
        // Drain the initial value so `has_changed` reflects only new sends.
        rx.mark_unchanged();
        ch.set(RelayHealth::Healthy); // same as current → no send
        assert!(!rx.has_changed().unwrap());

        ch.set(RelayHealth::Degraded {
            reason: REASON_NO_STREAM,
        });
        assert!(rx.has_changed().unwrap());
    }
}
