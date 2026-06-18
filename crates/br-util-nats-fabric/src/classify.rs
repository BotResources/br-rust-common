use crate::error::ConsumeErrorKind;

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

pub(crate) fn classify_create_consumer(
    err: &async_nats::jetstream::stream::ConsumerError,
) -> ConsumeErrorKind {
    use async_nats::jetstream::stream::ConsumerErrorKind as K;
    match err.kind() {
        K::JetStream(e) if e.error_code() == async_nats::jetstream::ErrorCode::STREAM_NOT_FOUND => {
            ConsumeErrorKind::NoStream
        }
        _ => ConsumeErrorKind::Other,
    }
}

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

pub(crate) fn classify_ack_error(
    err: &(dyn std::error::Error + Send + Sync + 'static),
) -> ConsumeErrorKind {
    use async_nats::client::PublishErrorKind as K;
    match err.downcast_ref::<async_nats::client::PublishError>() {
        Some(publish_err) if matches!(publish_err.kind(), K::Send) => {
            ConsumeErrorKind::ConsumerGone
        }
        _ => ConsumeErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_messages_error_kinds() {
        use async_nats::jetstream::consumer::pull::{MessagesError, MessagesErrorKind as K};
        let go = |k: K| classify_messages_error(&MessagesError::new(k));
        assert_eq!(go(K::ConsumerDeleted), ConsumeErrorKind::ConsumerGone);
        assert_eq!(go(K::MissingHeartbeat), ConsumeErrorKind::ConsumerGone);
        assert_eq!(go(K::NoResponders), ConsumeErrorKind::ConsumerGone);
        assert_eq!(go(K::Pull), ConsumeErrorKind::Other);
    }

    #[test]
    fn classifies_a_send_ack_error_as_consumer_gone() {
        let err = async_nats::client::PublishError::new(async_nats::client::PublishErrorKind::Send);
        assert_eq!(classify_ack_error(&err), ConsumeErrorKind::ConsumerGone);
    }

    #[test]
    fn classifies_an_unrelated_ack_error_as_other() {
        let err = std::io::Error::other("not a publish error");
        assert_eq!(classify_ack_error(&err), ConsumeErrorKind::Other);
    }
}
