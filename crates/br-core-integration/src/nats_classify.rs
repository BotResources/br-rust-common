use crate::ConsumeErrorKind;

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
