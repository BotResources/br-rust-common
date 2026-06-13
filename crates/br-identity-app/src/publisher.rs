use br_core_integration::{
    EventMetadata, IntegrationCommand, IntegrationEvent, IntegrationPublisher,
    IntegrationPublisherExt,
};
use br_core_scope::{
    ScopeDeclarationError, ServiceKey, ServiceScopesAccepted, ServiceScopesRejected,
};
use br_scope_declaration_contract::{VERSION, event_subject, event_type};
use chrono::Utc;
use uuid::Uuid;

use crate::error::AppError;

pub struct ConfirmationPublisher<P: IntegrationPublisher + ?Sized> {
    publisher: std::sync::Arc<P>,
}

impl<P: IntegrationPublisher + ?Sized> ConfirmationPublisher<P> {
    pub fn new(publisher: std::sync::Arc<P>) -> Self {
        Self { publisher }
    }

    pub async fn publish_accepted<T>(
        &self,
        command: &IntegrationCommand<T>,
        service: ServiceKey,
    ) -> Result<(), AppError> {
        let payload = ServiceScopesAccepted::new(service);
        self.publish("accepted", command, payload).await
    }

    pub async fn publish_rejected<T>(
        &self,
        command: &IntegrationCommand<T>,
        service: ServiceKey,
        reason: ScopeDeclarationError,
    ) -> Result<(), AppError> {
        let payload = ServiceScopesRejected::new(service, reason);
        self.publish("rejected", command, payload).await
    }

    async fn publish<T, Pay: serde::Serialize + Send + Sync>(
        &self,
        name: &str,
        command: &IntegrationCommand<T>,
        payload: Pay,
    ) -> Result<(), AppError> {
        let subject = event_subject(name).expect("static confirmation subject segments are valid");

        let metadata = EventMetadata::new(command.metadata.actor, command.metadata.correlation_id)
            .with_causation(command.command_id);

        let event = IntegrationEvent::new(
            Uuid::now_v7(),
            event_type(name),
            VERSION,
            Utc::now(),
            metadata,
            payload,
        );

        self.publisher.publish_event(&subject, &event).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_integration::{Actor, NoopIntegrationPublisher, UserId};
    use br_core_scope::DeclareServiceScopes;
    use std::sync::Arc;

    fn command() -> IntegrationCommand<DeclareServiceScopes> {
        let correlation = Uuid::now_v7();
        let metadata = EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation);
        IntegrationCommand::new(
            Uuid::now_v7(),
            "service_scope.declare",
            1,
            Utc::now(),
            metadata,
            serde_json::from_str(
                r#"{"declaration":{"manifest":{"key":"notifier","label_key":"l","description_key":"d"},"scopes":[]}}"#,
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn publish_accepted_builds_a_correlated_envelope() {
        let publisher = Arc::new(NoopIntegrationPublisher);
        let confirmations = ConfirmationPublisher::new(publisher);
        let cmd = command();
        confirmations
            .publish_accepted(&cmd, ServiceKey::new("notifier").unwrap())
            .await
            .expect("noop publish_accepted");
    }
}
