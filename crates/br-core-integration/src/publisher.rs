use crate::{IntegrationCommand, IntegrationError, IntegrationEvent};

#[async_trait::async_trait]
pub trait IntegrationPublisher: Send + Sync {
    async fn publish(
        &self,
        subject: &str,
        payload: serde_json::Value,
    ) -> Result<(), IntegrationError>;

    async fn publish_if_connected(&self, subject: &str, payload: serde_json::Value);
}

#[async_trait::async_trait]
pub trait IntegrationPublisherExt: IntegrationPublisher {
    async fn publish_event<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        event: &IntegrationEvent<T>,
    ) -> Result<(), IntegrationError> {
        let value = serde_json::to_value(event)?;
        self.publish(subject, value).await
    }

    async fn publish_command<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        command: &IntegrationCommand<T>,
    ) -> Result<(), IntegrationError> {
        let value = serde_json::to_value(command)?;
        self.publish(subject, value).await
    }

    async fn publish_event_if_connected<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        event: &IntegrationEvent<T>,
    ) {
        match serde_json::to_value(event) {
            Ok(value) => self.publish_if_connected(subject, value).await,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    subject = subject,
                    "integration event serialization failed; dropping"
                );
            }
        }
    }

    async fn publish_command_if_connected<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        command: &IntegrationCommand<T>,
    ) {
        match serde_json::to_value(command) {
            Ok(value) => self.publish_if_connected(subject, value).await,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    subject = subject,
                    "integration command serialization failed; dropping"
                );
            }
        }
    }
}

impl<P: IntegrationPublisher + ?Sized> IntegrationPublisherExt for P {}
