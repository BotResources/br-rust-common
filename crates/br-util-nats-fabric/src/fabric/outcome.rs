#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublishOutcome {
    Stored { sequence: u64 },
    Duplicate { sequence: u64 },
}

impl PublishOutcome {
    pub fn sequence(&self) -> u64 {
        match self {
            Self::Stored { sequence } | Self::Duplicate { sequence } => *sequence,
        }
    }

    pub fn is_duplicate(&self) -> bool {
        matches!(self, Self::Duplicate { .. })
    }

    pub(crate) fn from_ack(ack: &async_nats::jetstream::publish::PublishAck) -> Self {
        if ack.duplicate {
            Self::Duplicate {
                sequence: ack.sequence,
            }
        } else {
            Self::Stored {
                sequence: ack.sequence,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ack(sequence: u64, duplicate: bool) -> async_nats::jetstream::publish::PublishAck {
        serde_json::from_value(serde_json::json!({
            "stream": "INTEGRATION_EVT",
            "seq": sequence,
            "duplicate": duplicate,
        }))
        .expect("a PublishAck decodes from the broker wire shape")
    }

    #[test]
    fn a_plain_ack_is_stored_at_its_sequence() {
        let outcome = PublishOutcome::from_ack(&ack(42, false));
        assert_eq!(outcome, PublishOutcome::Stored { sequence: 42 });
        assert_eq!(outcome.sequence(), 42);
        assert!(!outcome.is_duplicate());
    }

    #[test]
    fn a_duplicate_ack_is_a_duplicate_at_the_stored_sequence() {
        let outcome = PublishOutcome::from_ack(&ack(42, true));
        assert_eq!(outcome, PublishOutcome::Duplicate { sequence: 42 });
        assert_eq!(outcome.sequence(), 42);
        assert!(outcome.is_duplicate());
    }
}
