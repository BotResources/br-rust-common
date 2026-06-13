use crate::{IntegrationError, IntegrationPublisher};

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
