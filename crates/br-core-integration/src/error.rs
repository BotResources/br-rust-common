#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublishErrorKind {
    NoStream,
    Timeout,
    Other,
}

impl std::fmt::Display for PublishErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::NoStream => "no stream for subject",
            Self::Timeout => "timed out",
            Self::Other => "publish failed",
        };
        f.write_str(s)
    }
}

impl From<async_nats::jetstream::context::PublishErrorKind> for PublishErrorKind {
    fn from(kind: async_nats::jetstream::context::PublishErrorKind) -> Self {
        use async_nats::jetstream::context::PublishErrorKind as Nats;
        match kind {
            Nats::StreamNotFound => Self::NoStream,
            Nats::TimedOut => Self::Timeout,
            _ => Self::Other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsumeErrorKind {
    NoStream,
    NoConsumer,
    ConsumerGone,
    Other,
}

impl std::fmt::Display for ConsumeErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::NoStream => "no such stream",
            Self::NoConsumer => "no such consumer",
            Self::ConsumerGone => "consumer gone",
            Self::Other => "consume failed",
        };
        f.write_str(s)
    }
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum IntegrationError {
    #[error("publish failed ({kind}): {detail}")]
    Publish {
        kind: PublishErrorKind,
        detail: String,
    },
    #[error("consume failed ({kind}): {detail}")]
    Consume {
        kind: ConsumeErrorKind,
        detail: String,
    },
    #[error("payload on '{subject}' failed to deserialize: {detail}")]
    Decode { subject: String, detail: String },
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl IntegrationError {
    pub(crate) fn from_publish(err: &async_nats::jetstream::context::PublishError) -> Self {
        Self::Publish {
            kind: err.kind().into(),
            detail: err.to_string(),
        }
    }

    pub(crate) fn consume(kind: ConsumeErrorKind, detail: impl Into<String>) -> Self {
        Self::Consume {
            kind,
            detail: detail.into(),
        }
    }

    pub(crate) fn decode(subject: impl Into<String>, err: &serde_json::Error) -> Self {
        Self::Decode {
            subject: subject.into(),
            detail: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_stream_not_found_as_no_stream() {
        use async_nats::jetstream::context::PublishErrorKind as Nats;
        assert_eq!(
            PublishErrorKind::from(Nats::StreamNotFound),
            PublishErrorKind::NoStream
        );
    }

    #[test]
    fn classifies_broken_pipe_as_other() {
        use async_nats::jetstream::context::PublishErrorKind as Nats;
        assert_eq!(
            PublishErrorKind::from(Nats::BrokenPipe),
            PublishErrorKind::Other
        );
    }

    #[test]
    fn classifies_timed_out_as_timeout() {
        use async_nats::jetstream::context::PublishErrorKind as Nats;
        assert_eq!(
            PublishErrorKind::from(Nats::TimedOut),
            PublishErrorKind::Timeout
        );
    }

    #[test]
    fn classifies_ambiguous_kinds_as_other() {
        use async_nats::jetstream::context::PublishErrorKind as Nats;
        assert_eq!(
            PublishErrorKind::from(Nats::MaxAckPending),
            PublishErrorKind::Other
        );
        assert_eq!(
            PublishErrorKind::from(Nats::WrongLastSequence),
            PublishErrorKind::Other
        );
    }

    #[test]
    fn publish_error_display_carries_kind_and_detail() {
        let err = IntegrationError::Publish {
            kind: PublishErrorKind::NoStream,
            detail: "no stream found for given subject".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("no stream for subject"));
        assert!(msg.contains("no stream found for given subject"));
    }

    #[test]
    fn serialization_error_from_serde_json() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{ not json");
        let err: IntegrationError = bad.unwrap_err().into();
        assert!(matches!(err, IntegrationError::Serialization(_)));
    }

    #[test]
    fn consume_error_display_carries_kind_and_detail() {
        let err = IntegrationError::consume(ConsumeErrorKind::NoStream, "stream IDENTITY missing");
        let msg = err.to_string();
        assert!(msg.contains("no such stream"));
        assert!(msg.contains("stream IDENTITY missing"));
    }

    #[test]
    fn decode_error_names_subject() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{ not json");
        let err = IntegrationError::decode("identity.evt.user.created.v1", &bad.unwrap_err());
        let msg = err.to_string();
        assert!(msg.contains("identity.evt.user.created.v1"));
    }

    #[test]
    fn consume_error_kinds_display_distinctly() {
        assert_eq!(ConsumeErrorKind::NoStream.to_string(), "no such stream");
        assert_eq!(ConsumeErrorKind::NoConsumer.to_string(), "no such consumer");
        assert_eq!(ConsumeErrorKind::ConsumerGone.to_string(), "consumer gone");
        assert_eq!(ConsumeErrorKind::Other.to_string(), "consume failed");
    }
}
