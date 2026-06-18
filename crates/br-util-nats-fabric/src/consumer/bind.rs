use async_nats::jetstream::consumer::PullConsumer;

use crate::classify::{classify_create_consumer, classify_get_stream};
use crate::consumer::config::{ConsumerTuning, durable_config};
use crate::error::FabricError;

pub(crate) async fn ensure_durable(
    jetstream: &async_nats::jetstream::Context,
    stream_name: &'static str,
    durable: &str,
    filter: &str,
    tuning: &ConsumerTuning,
) -> Result<PullConsumer, FabricError> {
    ensure_durable_many(
        jetstream,
        stream_name,
        durable,
        std::slice::from_ref(&filter),
        tuning,
    )
    .await
}

pub(crate) async fn ensure_durable_many(
    jetstream: &async_nats::jetstream::Context,
    stream_name: &'static str,
    durable: &str,
    filters: &[&str],
    tuning: &ConsumerTuning,
) -> Result<PullConsumer, FabricError> {
    if filters.is_empty() || filters.iter().all(|f| f.is_empty()) {
        return Err(FabricError::FilterMismatch {
            stream: stream_name,
            durable: durable.to_string(),
            expected: String::new(),
            configured: Vec::new(),
        });
    }

    let stream = jetstream
        .get_stream(stream_name)
        .await
        .map_err(|e| FabricError::consume(classify_get_stream(&e), e.to_string()))?;

    stream
        .create_consumer(durable_config(durable, filters, tuning))
        .await
        .map_err(|e| FabricError::consume(classify_create_consumer(&e), e.to_string()))
}
