//! JetStream-backed [`IntegrationPublisher`].

use crate::{IntegrationError, IntegrationPublisher};

/// JetStream-backed publisher. Wraps an [`async_nats::jetstream::Context`].
///
/// - [`publish`](IntegrationPublisher::publish) serializes the JSON value to
///   bytes, sends it on `subject`, and awaits the broker ack. Any ack error
///   (broker down, no-responders, no stream for the subject, etc.) surfaces as
///   [`IntegrationError::Publish`] with a classified
///   [`PublishErrorKind`](crate::PublishErrorKind).
/// - [`publish_if_connected`](IntegrationPublisher::publish_if_connected)
///   does the same but logs and swallows any error — useful for best-effort
///   side-channel emissions where the request handler must not fail because
///   the bus is unavailable.
pub struct NatsIntegrationPublisher {
    jetstream: async_nats::jetstream::Context,
}

impl NatsIntegrationPublisher {
    pub fn new(jetstream: async_nats::jetstream::Context) -> Self {
        Self { jetstream }
    }
}

#[async_trait::async_trait]
impl IntegrationPublisher for NatsIntegrationPublisher {
    async fn publish(
        &self,
        subject: &str,
        payload: serde_json::Value,
    ) -> Result<(), IntegrationError> {
        let bytes = serde_json::to_vec(&payload)?;
        // Two await points each return an `async_nats` `PublishError`; both are
        // classified through `from_publish`. The no-stream case surfaces at the
        // ack await as `StreamNotFound` → `PublishErrorKind::NoStream`.
        let ack = self
            .jetstream
            .publish(subject.to_string(), bytes.into())
            .await
            .map_err(|e| IntegrationError::from_publish(&e))?;
        ack.await.map_err(|e| IntegrationError::from_publish(&e))?;
        Ok(())
    }

    async fn publish_if_connected(&self, subject: &str, payload: serde_json::Value) {
        if let Err(err) = self.publish(subject, payload).await {
            tracing::warn!(
                error = %err,
                subject = subject,
                "integration publish failed; dropping"
            );
        }
    }
}
