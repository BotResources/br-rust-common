//! Errors returned by the publisher, with a classified publish-failure kind.

/// Why a publish failed, classified from the underlying `async_nats` publish
/// error so callers can branch (retry on `Timeout`, alert on `NoStream`)
/// without parsing a string.
///
/// `#[non_exhaustive]`: new kinds may be added as the transport surfaces new
/// failure modes, so always include a wildcard arm when matching.
///
/// ## Classification fidelity
///
/// async-nats 0.48 exposes its own `PublishErrorKind`; this maps it honestly,
/// and anything it cannot place with confidence becomes [`Other`](Self::Other)
/// rather than being guessed into a more specific kind:
///
/// | `async_nats::jetstream::context::PublishErrorKind` | maps to |
/// |---|---|
/// | `StreamNotFound`  | [`NoStream`](Self::NoStream) |
/// | `TimedOut`        | [`Timeout`](Self::Timeout) |
/// | everything else (incl. `BrokenPipe`) | [`Other`](Self::Other) |
///
/// `NoStream` is the one production-meaningful case: JetStream answers a
/// publish to a subject no stream captures with "no stream found", which is a
/// misconfiguration (the declared stream is missing) rather than a transient
/// fault — surfacing it distinctly lets a service fail loud on it. A broken
/// pipe (connection dropped mid-flight) is deliberately NOT given its own
/// kind: it carries no distinct branching value over `Other`, and naming it
/// something more specific (e.g. "no responders") would claim a cause the
/// transport did not actually report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PublishErrorKind {
    /// No JetStream stream is configured for the subject. A declared stream is
    /// missing — a misconfiguration, not a transient fault.
    NoStream,
    /// The broker did not acknowledge in time.
    Timeout,
    /// Any other or ambiguous transport failure (broken pipe, sequence
    /// conflicts, …).
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
            // `BrokenPipe`, `WrongLastMessageId`, `WrongLastSequence`,
            // `MaxAckPending`, and `Other` carry no extra branching value for
            // our callers and are honestly ambiguous as a transport outcome —
            // classify as `Other` rather than invent a more specific kind.
            _ => Self::Other,
        }
    }
}

/// Errors returned by [`IntegrationPublisher::publish`] and the typed helpers.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future variant is an
/// additive change.
///
/// [`IntegrationPublisher::publish`]: crate::IntegrationPublisher::publish
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum IntegrationError {
    /// A publish failed at the transport. `kind` classifies it; `detail`
    /// carries the underlying error text for logs.
    #[error("publish failed ({kind}): {detail}")]
    Publish {
        kind: PublishErrorKind,
        detail: String,
    },
    /// Encoding the message to JSON failed before any transport attempt.
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl IntegrationError {
    /// Build a [`Publish`](IntegrationError::Publish) from an async-nats publish
    /// error, classifying its kind and capturing its text as `detail`.
    pub(crate) fn from_publish(err: &async_nats::jetstream::context::PublishError) -> Self {
        Self::Publish {
            kind: err.kind().into(),
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

    // BrokenPipe is deliberately Other: a dropped connection is ambiguous and
    // naming it more specifically would claim a cause the transport did not
    // report.
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
}
