use crate::coords::{CommandCoords, EventCoords};
use crate::error::FabricError;
use crate::fabric::Fabric;

impl Fabric {
    pub async fn verify_command_durable(
        &self,
        coords: &CommandCoords,
        durable: &str,
    ) -> Result<(), FabricError> {
        self.ensure_command_durable(coords, durable).await
    }

    pub async fn verify_event_durable(
        &self,
        coords: &EventCoords,
        durable: &str,
    ) -> Result<(), FabricError> {
        self.ensure_event_durable(coords, durable).await
    }
}
