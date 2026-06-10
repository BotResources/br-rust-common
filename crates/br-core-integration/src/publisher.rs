//! The publishing interface: an object-safe core trait plus a blanket
//! extension trait of typed helpers.

use crate::{IntegrationCommand, IntegrationError, IntegrationEvent};

/// Object-safe publishing interface.
///
/// Hold as `Arc<dyn IntegrationPublisher>` from application code. For typed
/// publishing, bring [`IntegrationPublisherExt`] into scope.
#[async_trait::async_trait]
pub trait IntegrationPublisher: Send + Sync {
    /// Publish raw JSON to `subject` and wait for the broker ack.
    async fn publish(
        &self,
        subject: &str,
        payload: serde_json::Value,
    ) -> Result<(), IntegrationError>;

    /// Fire-and-forget publish: never returns an error. Implementations should
    /// log failures (e.g., the broker is down) but never propagate them.
    async fn publish_if_connected(&self, subject: &str, payload: serde_json::Value);
}

/// Typed helpers built on top of [`IntegrationPublisher`].
///
/// Auto-implemented for every `IntegrationPublisher` (including
/// `dyn IntegrationPublisher`) via the blanket `impl` below. Not object-safe
/// itself — the typed methods are monomorphized.
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
