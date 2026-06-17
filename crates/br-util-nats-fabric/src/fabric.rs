use serde::Serialize;

use br_core_integration::{IntegrationCommand, IntegrationEvent};

use crate::coords::{CommandCoords, EventCoords};
use crate::error::FabricError;

#[derive(Clone)]
pub struct Fabric {
    jetstream: async_nats::jetstream::Context,
}

impl Fabric {
    pub fn new(jetstream: async_nats::jetstream::Context) -> Self {
        Self { jetstream }
    }

    pub(crate) fn context(&self) -> &async_nats::jetstream::Context {
        &self.jetstream
    }

    pub async fn publish_command<T: Serialize>(
        &self,
        coords: &CommandCoords,
        command: &IntegrationCommand<T>,
    ) -> Result<(), FabricError> {
        self.publish(&coords.subject(), command).await
    }

    pub async fn publish_event<T: Serialize>(
        &self,
        coords: &EventCoords,
        event: &IntegrationEvent<T>,
    ) -> Result<(), FabricError> {
        self.publish(&coords.subject(), event).await
    }

    pub async fn publish_command_if_connected<T: Serialize>(
        &self,
        coords: &CommandCoords,
        command: &IntegrationCommand<T>,
    ) {
        self.publish_if_connected(coords.subject(), command).await;
    }

    pub async fn publish_event_if_connected<T: Serialize>(
        &self,
        coords: &EventCoords,
        event: &IntegrationEvent<T>,
    ) {
        self.publish_if_connected(coords.subject(), event).await;
    }

    #[cfg(feature = "outbox")]
    pub(crate) async fn publish_event_value(
        &self,
        coords: &EventCoords,
        payload: &serde_json::Value,
    ) -> Result<(), FabricError> {
        self.publish(&coords.subject(), payload).await
    }

    async fn publish<T: Serialize>(&self, subject: &str, envelope: &T) -> Result<(), FabricError> {
        let bytes = serde_json::to_vec(envelope)?;
        let ack = self
            .jetstream
            .publish(subject.to_string(), bytes.into())
            .await
            .map_err(|e| FabricError::from_publish(&e))?;
        ack.await.map_err(|e| FabricError::from_publish(&e))?;
        Ok(())
    }

    async fn publish_if_connected<T: Serialize>(&self, subject: String, envelope: &T) {
        if let Err(err) = self.publish(&subject, envelope).await {
            tracing::warn!(
                error = %err,
                subject = %subject,
                "fabric publish failed; dropping"
            );
        }
    }
}
