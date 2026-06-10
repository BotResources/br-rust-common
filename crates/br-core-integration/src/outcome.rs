//! The acknowledgement decision a handler returns for a delivered message.

use std::time::Duration;

/// What the durable consumer should tell JetStream about a handled message.
///
/// The handler decides; the [`DurableConsumer`](crate::DurableConsumer)
/// translates the decision into the matching JetStream ack on the wire. This
/// keeps the handler free of transport concerns while still surfacing the full
/// ack / nak / term contract.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future variant is an
/// additive change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageOutcome {
    /// The message was handled successfully; do not redeliver it.
    Ack,
    /// The message could not be handled now but should be retried. An optional
    /// delay defers redelivery (e.g. backing off a downstream dependency);
    /// `None` lets JetStream redeliver per the consumer's `AckWait`.
    Nak(Option<Duration>),
    /// The message must never be redelivered (it is unprocessable by this
    /// consumer — a poison message the handler recognises as such). This is
    /// the explicit alternative to an infinite nak loop.
    Term,
}

impl From<MessageOutcome> for async_nats::jetstream::AckKind {
    fn from(outcome: MessageOutcome) -> Self {
        match outcome {
            MessageOutcome::Ack => Self::Ack,
            MessageOutcome::Nak(delay) => Self::Nak(delay),
            MessageOutcome::Term => Self::Term,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_to_ack_kind() {
        assert!(matches!(
            async_nats::jetstream::AckKind::from(MessageOutcome::Ack),
            async_nats::jetstream::AckKind::Ack
        ));
        assert!(matches!(
            async_nats::jetstream::AckKind::from(MessageOutcome::Term),
            async_nats::jetstream::AckKind::Term
        ));
        let delay = Some(Duration::from_secs(5));
        assert!(matches!(
            async_nats::jetstream::AckKind::from(MessageOutcome::Nak(delay)),
            async_nats::jetstream::AckKind::Nak(Some(d)) if d == Duration::from_secs(5)
        ));
    }
}
