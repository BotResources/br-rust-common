use serde::de::DeserializeOwned;

use crate::consumer::bind::{bind_durable, bind_durable_many};
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
        self.bind_event_consumer_many(std::slice::from_ref(&coords), durable)
            .await
    }

    pub async fn bind_event_consumer_many<T: DeserializeOwned>(
        &self,
        coords: &[&EventCoords],
        durable: &str,
    ) -> Result<EventConsumer<T>, FabricError> {
        if coords.is_empty() {
            return Err(FabricError::FilterMismatch {
                stream: INTEGRATION_EVT,
                durable: durable.to_string(),
                expected: String::new(),
                configured: Vec::new(),
            });
        }
        let subjects: Vec<String> = coords.iter().map(|c| c.subject()).collect();
        let filters: Vec<&str> = subjects.iter().map(String::as_str).collect();
        let consumer =
            bind_durable_many(self.context(), INTEGRATION_EVT, durable, &filters).await?;
        EventConsumer::<T>::open_stream(consumer).await
    }
}
