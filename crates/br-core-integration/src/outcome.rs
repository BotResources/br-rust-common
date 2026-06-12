use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageOutcome {
    Ack,
    Nak(Option<Duration>),
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
