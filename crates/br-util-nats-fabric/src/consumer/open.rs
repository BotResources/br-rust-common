use serde::de::DeserializeOwned;

use crate::consumer::bind::{ensure_durable, ensure_durable_many};
use crate::consumer::bound::{CommandConsumer, EventConsumer};
use crate::consumer::config::ConsumerTuning;
use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::stream::{INTEGRATION_CMD, INTEGRATION_EVT};

impl Fabric {
    pub async fn ensure_command_consumer<T: DeserializeOwned>(
        &self,
        coords: &CommandCoords,
        durable: &str,
    ) -> Result<CommandConsumer<T>, FabricError> {
        self.ensure_command_consumer_with(coords, durable, &ConsumerTuning::default())
            .await
    }

    pub async fn ensure_command_consumer_with<T: DeserializeOwned>(
        &self,
        coords: &CommandCoords,
        durable: &str,
        tuning: &ConsumerTuning,
    ) -> Result<CommandConsumer<T>, FabricError> {
        let consumer = ensure_durable(
            self.context(),
            INTEGRATION_CMD,
            durable,
            &coords.subject(),
            tuning,
        )
        .await?;
        CommandConsumer::<T>::open_stream(consumer).await
    }

    pub async fn ensure_event_consumer<T: DeserializeOwned>(
        &self,
        coords: &EventCoords,
        durable: &str,
    ) -> Result<EventConsumer<T>, FabricError> {
        self.ensure_event_consumer_many(std::slice::from_ref(&coords), durable)
            .await
    }

    pub async fn ensure_event_consumer_with<T: DeserializeOwned>(
        &self,
        coords: &EventCoords,
        durable: &str,
        tuning: &ConsumerTuning,
    ) -> Result<EventConsumer<T>, FabricError> {
        self.ensure_event_consumer_many_with(std::slice::from_ref(&coords), durable, tuning)
            .await
    }

    pub async fn ensure_event_consumer_many<T: DeserializeOwned>(
        &self,
        coords: &[&EventCoords],
        durable: &str,
    ) -> Result<EventConsumer<T>, FabricError> {
        self.ensure_event_consumer_many_with(coords, durable, &ConsumerTuning::default())
            .await
    }

    pub async fn ensure_event_consumer_many_with<T: DeserializeOwned>(
        &self,
        coords: &[&EventCoords],
        durable: &str,
        tuning: &ConsumerTuning,
    ) -> Result<EventConsumer<T>, FabricError> {
        let subjects: Vec<String> = coords.iter().map(|c| c.subject()).collect();
        let filters: Vec<&str> = subjects.iter().map(String::as_str).collect();
        let consumer =
            ensure_durable_many(self.context(), INTEGRATION_EVT, durable, &filters, tuning).await?;
        EventConsumer::<T>::open_stream(consumer).await
    }
}
