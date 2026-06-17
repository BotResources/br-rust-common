use std::future::Future;

use async_nats::jetstream::consumer::PullConsumer;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use br_core_integration::{IntegrationCommand, IntegrationEvent, MessageOutcome};

use crate::classify::classify_messages_error;
use crate::consumer::bind::bind_durable;
use crate::coords::{CommandCoords, EventCoords};
use crate::error::{ConsumeErrorKind, FabricError};
use crate::fabric::Fabric;
use crate::stream::{INTEGRATION_CMD, INTEGRATION_EVT};

#[non_exhaustive]
pub struct Delivery<E> {
    pub subject: String,
    pub envelope: E,
}

impl Fabric {
    pub async fn run_commands<T, H, HFut, P>(
        &self,
        coords: &CommandCoords,
        durable: &str,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationCommand<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        let consumer =
            bind_durable(self.context(), INTEGRATION_CMD, durable, &coords.subject()).await?;
        run_inner::<IntegrationCommand<T>, _, _, _>(consumer, &mut handler, &mut on_poison).await
    }

    pub async fn run_events<T, H, HFut, P>(
        &self,
        coords: &EventCoords,
        durable: &str,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationEvent<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        let consumer =
            bind_durable(self.context(), INTEGRATION_EVT, durable, &coords.subject()).await?;
        run_inner::<IntegrationEvent<T>, _, _, _>(consumer, &mut handler, &mut on_poison).await
    }
}

async fn run_inner<E, H, HFut, P>(
    consumer: PullConsumer,
    handler: &mut H,
    on_poison: &mut P,
) -> Result<(), FabricError>
where
    E: DeserializeOwned,
    H: FnMut(Delivery<E>) -> HFut,
    HFut: Future<Output = MessageOutcome>,
    P: FnMut(FabricError),
{
    let mut messages = consumer
        .messages()
        .await
        .map_err(|e| FabricError::consume(ConsumeErrorKind::Other, e.to_string()))?;

    while let Some(message) = messages.next().await {
        let message = message
            .map_err(|e| FabricError::consume(classify_messages_error(&e), e.to_string()))?;
        let subject = message.subject.as_str().to_string();

        match serde_json::from_slice::<E>(&message.payload) {
            Ok(envelope) => {
                let outcome = handler(Delivery { subject, envelope }).await;
                apply_outcome(&message, outcome).await;
            }
            Err(decode_err) => {
                let _ = message.ack_with(async_nats::jetstream::AckKind::Term).await;
                on_poison(FabricError::decode(subject, &decode_err));
            }
        }
    }

    Ok(())
}

async fn apply_outcome(message: &async_nats::jetstream::Message, outcome: MessageOutcome) {
    if let Err(err) = message.ack_with(outcome.into()).await {
        tracing::warn!(
            error = %err,
            subject = %message.subject,
            ?outcome,
            "failed to send ack for handled message; it may be redelivered"
        );
    }
}
