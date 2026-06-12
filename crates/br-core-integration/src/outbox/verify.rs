use crate::{ConsumeErrorKind, IntegrationError};

pub async fn verify_consumer(
    jetstream: &async_nats::jetstream::Context,
    stream: &str,
    consumer: &str,
) -> Result<(), IntegrationError> {
    let stream_handle = jetstream
        .get_stream(stream)
        .await
        .map_err(|e| IntegrationError::consume(ConsumeErrorKind::NoStream, e.to_string()))?;
    stream_handle
        .consumer_info(consumer)
        .await
        .map_err(|e| IntegrationError::consume(ConsumeErrorKind::NoConsumer, e.to_string()))?;
    Ok(())
}
