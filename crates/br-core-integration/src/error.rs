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

/// Why binding or running a consumer failed, classified so a caller can fail
/// loud on a missing declared object (the lib never auto-provisions) versus
/// retry a transient transport fault.
///
/// `#[non_exhaustive]`: new kinds may be added as the transport surfaces new
/// failure modes, so always include a wildcard arm when matching.
///
/// `NoStream` / `NoConsumer` are the production-meaningful cases at bind time: a
/// declared JetStream object is missing, which is a misconfiguration (fail loud,
/// do not auto-create) rather than a transient fault. [`ConsumerGone`](Self::ConsumerGone)
/// is the production-meaningful case while *pulling*: the consumer the wrapper or
/// awaiter was reading vanished mid-run (deleted server-side, or missed
/// heartbeats), so the message stream ended and the shape fails loud rather than
/// silently spinning. Everything the transport cannot place with confidence
/// becomes [`Other`](Self::Other) rather than being guessed into a more specific
/// kind.
///
/// ## Classification fidelity (the pull message stream)
///
/// async-nats 0.48's pull `messages()` stream yields `Result<_, MessagesError>`;
/// when it ends or errors mid-run, `classify_messages_error` maps its
/// `MessagesErrorKind` honestly:
///
/// | `async_nats::jetstream::consumer::pull::MessagesErrorKind` | maps to |
/// |---|---|
/// | `ConsumerDeleted`, `MissingHeartbeat`, `NoResponders` | [`ConsumerGone`](Self::ConsumerGone) |
/// | everything else (`Pull`, `PushBasedConsumer`, `Other`) | [`Other`](Self::Other) |
///
/// `NoResponders` is the `503` a pull request gets when no consumer answers it —
/// for an ephemeral awaiter reaped past its `inactive_threshold`, the kind
/// actually observed on the next pull — so it belongs with the consumer-gone
/// cluster (async-nats' own ordered-consumer recovery groups these three as
/// "recreate the consumer").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsumeErrorKind {
    /// The named JetStream stream does not exist. A declared stream is missing
    /// — a misconfiguration, not a transient fault. The lib never creates it.
    NoStream,
    /// The named durable consumer does not exist on the stream. A declared
    /// consumer is missing — a misconfiguration, not a transient fault. The
    /// durable wrapper never creates it.
    NoConsumer,
    /// The consumer being pulled vanished mid-run: deleted server-side or its
    /// heartbeats stopped, so the message stream ended. For the ephemeral
    /// awaiter this is typically the server reaping it past its
    /// `inactive_threshold` between waits; for the durable wrapper it is the
    /// bound consumer being deleted while running. Fail loud — never silently
    /// spin on a dead stream.
    ConsumerGone,
    /// Any other or ambiguous transport / request failure while binding or
    /// pulling messages (broker unreachable, request timeout, …).
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

/// Errors returned by [`IntegrationPublisher::publish`], the typed helpers, and
/// the consumer shapes (the durable wrapper and the correlated awaiter).
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
    /// Binding to a stream/consumer or pulling a message failed. `kind`
    /// classifies it (`NoStream` / `NoConsumer` are fail-loud
    /// misconfigurations); `detail` carries the underlying error text for logs.
    #[error("consume failed ({kind}): {detail}")]
    Consume {
        kind: ConsumeErrorKind,
        detail: String,
    },
    /// A delivered message could not be deserialized into the expected typed
    /// envelope — a poison message. `subject` is where it arrived, `detail`
    /// the serde error. The consumer shapes surface this rather than silently
    /// dropping the message; the durable wrapper additionally `term`s it so it
    /// is not redelivered forever.
    #[error("payload on '{subject}' failed to deserialize: {detail}")]
    Decode { subject: String, detail: String },
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

    /// Build a [`Consume`](IntegrationError::Consume) with an explicit `kind`,
    /// capturing `detail` for logs.
    pub(crate) fn consume(kind: ConsumeErrorKind, detail: impl Into<String>) -> Self {
        Self::Consume {
            kind,
            detail: detail.into(),
        }
    }

    /// Build a [`Decode`](IntegrationError::Decode) poison-message error.
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
