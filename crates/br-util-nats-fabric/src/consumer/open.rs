use serde::de::DeserializeOwned;

use crate::consumer::bind::bind_durable;
use crate::consumer::bound::{CommandConsumer, EventConsumer};
use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::stream::{INTEGRATION_CMD, INTEGRATION_EVT};

impl Fabric {
    pub async fn bind_command_consumer<T: DeserializeOwned>(
        &self,
        coords: &CommandCoords,
        durable: &str,
    ) -> Result<CommandConsumer<T>, FabricError> {
        let consumer =
            bind_durable(self.context(), INTEGRATION_CMD, durable, &coords.subject()).await?;
        CommandConsumer::<T>::open_stream(consumer).await
    }

    pub async fn bind_event_consumer<T: DeserializeOwned>(
        &self,
        coords: &EventCoords,
        durable: &str,
    ) -> Result<EventConsumer<T>, FabricError> {
        let consumer =
            bind_durable(self.context(), INTEGRATION_EVT, durable, &coords.subject()).await?;
        EventConsumer::<T>::open_stream(consumer).await
    }
}
