use crate::consumer::bind::bind_durable;
use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::stream::{INTEGRATION_CMD, INTEGRATION_EVT};

impl Fabric {
    pub async fn verify_command_durable(
        &self,
        coords: &CommandCoords,
        durable: &str,
    ) -> Result<(), FabricError> {
        bind_durable(self.context(), INTEGRATION_CMD, durable, &coords.subject()).await?;
        Ok(())
    }

    pub async fn verify_event_durable(
        &self,
        coords: &EventCoords,
        durable: &str,
    ) -> Result<(), FabricError> {
        bind_durable(self.context(), INTEGRATION_EVT, durable, &coords.subject()).await?;
        Ok(())
    }
}
