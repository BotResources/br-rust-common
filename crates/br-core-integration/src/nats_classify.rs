//! Maps async-nats 0.48 error *kinds* to our [`ConsumeErrorKind`], so the
//! consumer shapes can fail loud on a missing/vanished declared object versus
//! retry a transient transport fault. One responsibility: translate the
//! transport's taxonomy into ours, honestly (anything ambiguous → `Other`,
//! never guessed into a more specific kind).

use crate::ConsumeErrorKind;

/// Classify a `GetStreamError`: a JetStream `STREAM_NOT_FOUND` (10059) is the
/// fail-loud [`NoStream`](ConsumeErrorKind::NoStream); anything else is
/// transient/ambiguous [`Other`](ConsumeErrorKind::Other).
pub(crate) fn classify_get_stream(
    err: &async_nats::jetstream::context::GetStreamError,
) -> ConsumeErrorKind {
    use async_nats::jetstream::context::GetStreamErrorKind as K;
    match err.kind() {
        K::JetStream(e) if e.error_code() == async_nats::jetstream::ErrorCode::STREAM_NOT_FOUND => {
            ConsumeErrorKind::NoStream
        }
        _ => ConsumeErrorKind::Other,
    }
}

/// Classify a `ConsumerInfoError` (from binding a durable consumer by name): a
/// missing consumer is the fail-loud [`NoConsumer`](ConsumeErrorKind::NoConsumer),
/// a missing stream is [`NoStream`](ConsumeErrorKind::NoStream); anything else
/// is `Other`. Both the typed `NotFound` variant and a JetStream
/// `CONSUMER_NOT_FOUND` (10014) are treated as a missing consumer, since the
/// transport surfaces it either way depending on the request path.
pub(crate) fn classify_consumer_info(
    err: &async_nats::jetstream::context::ConsumerInfoError,
) -> ConsumeErrorKind {
    use async_nats::jetstream::context::ConsumerInfoErrorKind as K;
    match err.kind() {
        K::NotFound => ConsumeErrorKind::NoConsumer,
        K::StreamNotFound => ConsumeErrorKind::NoStream,
        K::JetStream(e)
            if e.error_code() == async_nats::jetstream::ErrorCode::CONSUMER_NOT_FOUND =>
        {
            ConsumeErrorKind::NoConsumer
        }
        K::JetStream(e) if e.error_code() == async_nats::jetstream::ErrorCode::STREAM_NOT_FOUND => {
            ConsumeErrorKind::NoStream
        }
        _ => ConsumeErrorKind::Other,
    }
}

/// Classify a pull `MessagesError` surfaced while reading the message stream
/// (a `Some(Err(_))` yielded by `next()`).
///
/// `ConsumerDeleted`, `MissingHeartbeat`, and `NoResponders` all mean the
/// consumer the stream is bound to vanished mid-run →
/// [`ConsumerGone`](ConsumeErrorKind::ConsumerGone). The first two are
/// self-evident; `NoResponders` is the `503` JetStream returns to a pull request
/// that no consumer answers — for an ephemeral consumer reaped past its
/// `inactive_threshold` this is the kind actually observed when the next pull
/// fires. async-nats' own ordered-consumer recovery groups exactly these three
/// as "the consumer must be recreated", which is the authority for treating them
/// alike here. Everything else (`Pull`, `PushBasedConsumer`, `Other`) is
/// transient/ambiguous [`Other`](ConsumeErrorKind::Other).
pub(crate) fn classify_messages_error(
    err: &async_nats::jetstream::consumer::pull::MessagesError,
) -> ConsumeErrorKind {
    use async_nats::jetstream::consumer::pull::MessagesErrorKind as K;
    match err.kind() {
        K::ConsumerDeleted | K::MissingHeartbeat | K::NoResponders => {
            ConsumeErrorKind::ConsumerGone
        }
        _ => ConsumeErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A consumer that vanished mid-run (deleted server-side, or stopped sending
    // heartbeats) is the fail-loud `ConsumerGone`; a pull-request failure or any
    // other kind stays the ambiguous `Other`.
    #[test]
    fn classifies_messages_error_kinds() {
        use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind as K};
        let go = |k: K| classify_messages_error(&MessagesError::new(k));
        assert_eq!(go(K::ConsumerDeleted), ConsumeErrorKind::ConsumerGone);
        assert_eq!(go(K::MissingHeartbeat), ConsumeErrorKind::ConsumerGone);
        assert_eq!(go(K::NoResponders), ConsumeErrorKind::ConsumerGone);
        assert_eq!(go(K::Pull), ConsumeErrorKind::Other);
        assert_eq!(go(K::PushBasedConsumer), ConsumeErrorKind::Other);
        assert_eq!(go(K::Other), ConsumeErrorKind::Other);
    }
}
