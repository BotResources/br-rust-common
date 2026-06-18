use crate::coords::CoordError;
use crate::kv::KvKeyError;

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
    NoDeliveryInfo,
}

impl std::fmt::Display for ConsumeErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::NoStream => "no such stream",
            Self::NoConsumer => "no such consumer",
            Self::ConsumerGone => "consumer gone",
            Self::NoDeliveryInfo => "delivery info absent",
            Self::Other => "consume failed",
        };
        f.write_str(s)
    }
}

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum FabricError {
    #[error("connection to NATS failed: {0}")]
    Connect(String),
    #[error("invalid coordinate: {0}")]
    Coord(#[from] CoordError),
    #[error("invalid kv key: {0}")]
    KvKey(#[from] KvKeyError),
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
    #[error(
        "durable '{durable}' on stream '{stream}' filters {configured:?}, expected exactly [{expected}]"
    )]
    FilterMismatch {
        stream: &'static str,
        durable: String,
        expected: String,
        configured: Vec<String>,
    },
    #[error("payload on '{subject}' failed to deserialize: {detail}")]
    Decode { subject: String, detail: String },
    #[error("revision conflict on kv key '{key}' (expected {expected})")]
    RevisionConflict { key: String, expected: u64 },
    #[error("kv key '{key}' already exists")]
    KeyAlreadyExists { key: String },
    #[error("key/value store error: {0}")]
    Kv(String),
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl FabricError {
    pub(crate) fn connect(err: &async_nats::ConnectError) -> Self {
        Self::Connect(err.to_string())
    }

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

    pub(crate) fn kv(detail: impl std::fmt::Display) -> Self {
        Self::Kv(detail.to_string())
    }

    pub(crate) fn revision_conflict(key: impl Into<String>, expected: u64) -> Self {
        Self::RevisionConflict {
            key: key.into(),
            expected,
        }
    }

    pub(crate) fn key_already_exists(key: impl Into<String>) -> Self {
        Self::KeyAlreadyExists { key: key.into() }
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
    fn classifies_timed_out_as_timeout() {
        use async_nats::jetstream::context::PublishErrorKind as Nats;
        assert_eq!(
            PublishErrorKind::from(Nats::TimedOut),
            PublishErrorKind::Timeout
        );
    }

    #[test]
    fn classifies_other_publish_kinds_as_other() {
        use async_nats::jetstream::context::PublishErrorKind as Nats;
        assert_eq!(
            PublishErrorKind::from(Nats::BrokenPipe),
            PublishErrorKind::Other
        );
    }

    #[test]
    fn consume_kinds_display_distinctly() {
        assert_eq!(ConsumeErrorKind::NoStream.to_string(), "no such stream");
        assert_eq!(ConsumeErrorKind::NoConsumer.to_string(), "no such consumer");
        assert_eq!(ConsumeErrorKind::ConsumerGone.to_string(), "consumer gone");
    }

    #[test]
    fn coord_error_maps_into_fabric_error() {
        let err: FabricError = crate::coords::Bc::new("bad.bc").unwrap_err().into();
        assert!(matches!(err, FabricError::Coord(_)));
    }

    #[test]
    fn filter_mismatch_names_stream_durable_and_expected() {
        let err = FabricError::FilterMismatch {
            stream: "INTEGRATION_EVT",
            durable: "svc-pm-users".to_string(),
            expected: "integration.evt.identity.user.created.v1".to_string(),
            configured: vec!["integration.evt.>".to_string()],
        };
        let msg = err.to_string();
        assert!(msg.contains("INTEGRATION_EVT"));
        assert!(msg.contains("svc-pm-users"));
        assert!(msg.contains("integration.evt.identity.user.created.v1"));
    }

    #[test]
    fn serialization_error_from_serde_json() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{ not json");
        let err: FabricError = bad.unwrap_err().into();
        assert!(matches!(err, FabricError::Serialization(_)));
    }
}
