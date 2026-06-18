use serde::Serialize;

use br_core_integration::{IntegrationCommand, IntegrationEvent};

use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::FabricError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionState {
    Pending,
    Connected,
    Disconnected,
}

impl From<async_nats::connection::State> for ConnectionState {
    fn from(state: async_nats::connection::State) -> Self {
        use async_nats::connection::State as S;
        match state {
            S::Pending => Self::Pending,
            S::Connected => Self::Connected,
            S::Disconnected => Self::Disconnected,
        }
    }
}

#[derive(Clone)]
pub struct NatsAuth {
    pub user: String,
    pub password: String,
}

impl std::fmt::Debug for NatsAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NatsAuth")
            .field("user", &self.user)
            .field("password", &"***")
            .finish()
    }
}

#[derive(Clone)]
pub struct Fabric {
    jetstream: async_nats::jetstream::Context,
}

impl Fabric {
    pub fn new(jetstream: async_nats::jetstream::Context) -> Self {
        Self { jetstream }
    }

    pub async fn connect(url: &str) -> Result<Self, FabricError> {
        let client = async_nats::connect(url)
            .await
            .map_err(|e| FabricError::connect(&e))?;
        Ok(Self::new(async_nats::jetstream::new(client)))
    }

    pub async fn connect_with(url: &str, auth: &NatsAuth) -> Result<Self, FabricError> {
        let client = async_nats::ConnectOptions::with_user_and_password(
            auth.user.clone(),
            auth.password.clone(),
        )
        .connect(url)
        .await
        .map_err(|e| FabricError::connect(&e))?;
        Ok(Self::new(async_nats::jetstream::new(client)))
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

    pub async fn publish_command_with_id<T: Serialize>(
        &self,
        coords: &CommandCoords,
        command: &IntegrationCommand<T>,
        message_id: &str,
    ) -> Result<(), FabricError> {
        self.publish_with_id(&coords.subject(), command, message_id)
            .await
    }

    pub async fn publish_event_with_id<T: Serialize>(
        &self,
        coords: &EventCoords,
        event: &IntegrationEvent<T>,
        message_id: &str,
    ) -> Result<(), FabricError> {
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
    pub(crate) async fn publish_event_value(
        &self,
        coords: &EventCoords,
        payload: &serde_json::Value,
    ) -> Result<(), FabricError> {
        self.publish(&coords.subject(), payload).await
    }

    pub fn connection_state(&self) -> ConnectionState {
        self.jetstream.client().connection_state().into()
    }

    pub fn reachable(&self) -> bool {
        self.connection_state() == ConnectionState::Connected
    }

    pub async fn ping(&self) -> Result<(), FabricError> {
        self.jetstream
            .client()
            .flush()
            .await
            .map_err(|e| FabricError::Connect(e.to_string()))
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

    async fn publish_with_id<T: Serialize>(
        &self,
        subject: &str,
        envelope: &T,
        message_id: &str,
    ) -> Result<(), FabricError> {
        let bytes = serde_json::to_vec(envelope)?;
        let mut headers = async_nats::HeaderMap::new();
        headers.insert(async_nats::header::NATS_MESSAGE_ID, message_id);
        let ack = self
            .jetstream
            .publish_with_headers(subject.to_string(), headers, bytes.into())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nats_auth_debug_masks_the_password_and_never_prints_it() {
        let auth = NatsAuth {
            user: "fabric".to_string(),
            password: "s3cr3t-rotation-key".to_string(),
        };
        let rendered = format!("{auth:?}");
        assert!(
            !rendered.contains("s3cr3t-rotation-key"),
            "Debug leaked the password: {rendered}"
        );
        assert!(
            rendered.contains("***"),
            "Debug must mask the password with ***: {rendered}"
        );
        assert!(
            rendered.contains("fabric"),
            "Debug keeps the user for diagnostics: {rendered}"
        );
    }
}
