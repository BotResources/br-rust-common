use crate::consumer::bind::ensure_durable;
use crate::consumer::config::ConsumerTuning;
use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::stream::{INTEGRATION_CMD, INTEGRATION_EVT};

impl Fabric {
    pub async fn ensure_command_durable(
        &self,
        coords: &CommandCoords,
        durable: &str,
    ) -> Result<(), FabricError> {
        self.ensure_command_durable_with(coords, durable, &ConsumerTuning::default())
            .await
    }

    pub async fn ensure_command_durable_with(
        &self,
        coords: &CommandCoords,
        durable: &str,
        tuning: &ConsumerTuning,
    ) -> Result<(), FabricError> {
        ensure_durable(
            self.context(),
            INTEGRATION_CMD,
            durable,
            &coords.subject(),
            tuning,
        )
        .await?;
        Ok(())
    }

    pub async fn ensure_event_durable(
        &self,
        coords: &EventCoords,
        durable: &str,
    ) -> Result<(), FabricError> {
        self.ensure_event_durable_with(coords, durable, &ConsumerTuning::default())
            .await
    }

    pub async fn ensure_event_durable_with(
        &self,
        coords: &EventCoords,
        durable: &str,
        tuning: &ConsumerTuning,
    ) -> Result<(), FabricError> {
        ensure_durable(
            self.context(),
            INTEGRATION_EVT,
            durable,
            &coords.subject(),
            tuning,
        )
        .await?;
        Ok(())
    }
}
