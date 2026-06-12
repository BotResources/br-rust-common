//! Receiver-online precheck — **not** part of the feature-gated outbox.
//!
//! [`verify_consumer`] touches only `async_nats` (no `sqlx`), so it must be
//! callable **without** the `outbox` feature: a service that issues a critical
//! command needs the precheck whether or not it stages outbox rows. Keeping it
//! in this ungated module is what makes that true — it sits next to the outbox
//! conceptually (it is the medisup seed's `check_consumer`, made honest) but
//! shares none of the store/relay's Postgres surface.

use crate::{ConsumeErrorKind, IntegrationError};

/// Verify a durable consumer exists on `stream` before publishing — the honest
/// form of the medisup seed's `check_consumer`: a fail-fast for a critical
/// command whose receiver must be online (e.g. no worker is bound, so the
/// command would sit unconsumed). Returns
/// [`ConsumeErrorKind::NoConsumer`](crate::ConsumeErrorKind::NoConsumer) when the
/// consumer is absent, classified through the same layer the consumer shapes use.
///
/// This is **opt-in and separate from the relay**: it never auto-provisions, and
/// most events do not need it (a fact is published whether or not a subscriber
/// is currently online). Call it from the staging path when, and only when, the
/// receiver's presence is a precondition for issuing the command.
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
