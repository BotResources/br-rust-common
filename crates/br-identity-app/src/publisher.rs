use br_core_integration::{EventCoords, EventMetadata, IntegrationCommand, IntegrationEvent};
use br_core_scope::{
    ScopeDeclarationError, ServiceKey, ServiceScopesAccepted, ServiceScopesRejected,
};
use br_scope_declaration_contract::{
    ACCEPTED, REJECTED, VERSION, accepted_event_coords, event_type, rejected_event_coords,
};
use br_util_nats_fabric::Fabric;
use chrono::Utc;
use uuid::Uuid;

use crate::error::AppError;

pub struct ConfirmationPublisher {
    fabric: Fabric,
}

impl ConfirmationPublisher {
    pub fn new(fabric: Fabric) -> Self {
        Self { fabric }
    }

    pub async fn publish_accepted<T>(
        &self,
        command: &IntegrationCommand<T>,
        service: ServiceKey,
    ) -> Result<(), AppError> {
        let coords = accepted_event_coords().expect("contract coordinates are valid");
        let payload = ServiceScopesAccepted::new(service);
        self.publish(ACCEPTED, &coords, command, payload).await
    }

    pub async fn publish_rejected<T>(
        &self,
        command: &IntegrationCommand<T>,
        service: ServiceKey,
        reason: ScopeDeclarationError,
    ) -> Result<(), AppError> {
        let coords = rejected_event_coords().expect("contract coordinates are valid");
        let payload = ServiceScopesRejected::new(service, reason);
        self.publish(REJECTED, &coords, command, payload).await
    }

    async fn publish<T, Pay: serde::Serialize + Send + Sync>(
        &self,
        name: &str,
        coords: &EventCoords,
        command: &IntegrationCommand<T>,
        payload: Pay,
    ) -> Result<(), AppError> {
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

        self.fabric.publish_event(coords, &event).await?;
        Ok(())
    }
}
