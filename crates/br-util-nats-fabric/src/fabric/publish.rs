use serde::Serialize;

use br_core_integration::{IntegrationCommand, IntegrationEvent};

use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::FabricError;
use crate::fabric::{Fabric, PublishOutcome};

impl Fabric {
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

    pub async fn publish_command_with_id<T: Serialize>(
        &self,
        coords: &CommandCoords,
        command: &IntegrationCommand<T>,
        message_id: &str,
    ) -> Result<(), FabricError> {
        self.publish_command_with_id_outcome(coords, command, message_id)
            .await
            .map(|_| ())
    }

    pub async fn publish_event_with_id<T: Serialize>(
        &self,
        coords: &EventCoords,
        event: &IntegrationEvent<T>,
        message_id: &str,
    ) -> Result<(), FabricError> {
        self.publish_event_with_id_outcome(coords, event, message_id)
            .await
            .map(|_| ())
    }

    pub async fn publish_command_with_id_outcome<T: Serialize>(
        &self,
        coords: &CommandCoords,
        command: &IntegrationCommand<T>,
        message_id: &str,
    ) -> Result<PublishOutcome, FabricError> {
        self.publish_with_id(&coords.subject(), command, message_id)
            .await
    }

    pub async fn publish_event_with_id_outcome<T: Serialize>(
        &self,
        coords: &EventCoords,
        event: &IntegrationEvent<T>,
        message_id: &str,
    ) -> Result<PublishOutcome, FabricError> {
        self.publish_with_id(&coords.subject(), event, message_id)
            .await
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
    pub(crate) async fn publish_event_value_with_id(
        &self,
        coords: &EventCoords,
        payload: &serde_json::Value,
        message_id: &str,
    ) -> Result<PublishOutcome, FabricError> {
        self.publish_with_id(&coords.subject(), payload, message_id)
            .await
    }

    async fn publish<T: Serialize>(&self, subject: &str, envelope: &T) -> Result<(), FabricError> {
        let bytes = serde_json::to_vec(envelope)?;
        let ack = self
            .context()
            .publish(subject.to_string(), bytes.into())
            .await
            .map_err(|e| FabricError::from_publish(&e))?;
        ack.await.map_err(|e| FabricError::from_publish(&e))?;
        Ok(())
    }

    async fn publish_with_id<T: Serialize>(
        &self,
        subject: &str,
        envelope: &T,
        message_id: &str,
    ) -> Result<PublishOutcome, FabricError> {
        let bytes = serde_json::to_vec(envelope)?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert(async_nats::header::NATS_MESSAGE_ID, message_id);
        let ack = self
            .context()
            .publish_with_headers(subject.to_string(), headers, bytes.into())
            .await
            .map_err(|e| FabricError::from_publish(&e))?;
        let ack = ack.await.map_err(|e| FabricError::from_publish(&e))?;
        Ok(PublishOutcome::from_ack(&ack))
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
