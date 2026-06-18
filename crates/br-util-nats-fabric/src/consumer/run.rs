use std::future::Future;

use async_nats::jetstream::consumer::PullConsumer;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use br_core_integration::{IntegrationCommand, IntegrationEvent, MessageOutcome};

use crate::classify::classify_messages_error;
use crate::consumer::bind::ensure_durable;
use crate::consumer::config::ConsumerTuning;
use crate::coords::{CommandCoords, EventCoords, IntegrationSubject};
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
        handler: H,
        on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationCommand<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        self.run_commands_with(
            coords,
            durable,
            &ConsumerTuning::default(),
            handler,
            on_poison,
        )
        .await
    }

    pub async fn run_commands_with<T, H, HFut, P>(
        &self,
        coords: &CommandCoords,
        durable: &str,
        tuning: &ConsumerTuning,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationCommand<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        let consumer = ensure_durable(
            self.context(),
            INTEGRATION_CMD,
            durable,
            &coords.subject(),
            tuning,
        )
        .await?;
        run_inner::<IntegrationCommand<T>, _, _, _>(consumer, &mut handler, &mut on_poison).await
    }

    pub async fn run_events<T, H, HFut, P>(
        &self,
        coords: &EventCoords,
        durable: &str,
        handler: H,
        on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationEvent<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        self.run_events_with(
            coords,
            durable,
            &ConsumerTuning::default(),
            handler,
            on_poison,
        )
        .await
    }

    pub async fn run_events_with<T, H, HFut, P>(
        &self,
        coords: &EventCoords,
        durable: &str,
        tuning: &ConsumerTuning,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), FabricError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationEvent<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(FabricError),
    {
        let consumer = ensure_durable(
            self.context(),
            INTEGRATION_EVT,
            durable,
            &coords.subject(),
            tuning,
        )
        .await?;
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

fn ack_kind(outcome: MessageOutcome) -> async_nats::jetstream::AckKind {
    match outcome {
        MessageOutcome::Ack => async_nats::jetstream::AckKind::Ack,
        MessageOutcome::Nak(delay) => async_nats::jetstream::AckKind::Nak(delay),
        MessageOutcome::Term => async_nats::jetstream::AckKind::Term,
        _ => async_nats::jetstream::AckKind::Nak(None),
    }
}

async fn apply_outcome(message: &async_nats::jetstream::Message, outcome: MessageOutcome) {
    if let Err(err) = message.ack_with(ack_kind(outcome)).await {
        tracing::warn!(
            error = %err,
            subject = %message.subject,
            ?outcome,
            "failed to send ack for handled message; it may be redelivered"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::ack_kind;
    use async_nats::jetstream::AckKind;
    use br_core_integration::MessageOutcome;
    use std::time::Duration;

    #[test]
    fn maps_known_outcomes_to_their_ack_kind() {
        assert!(matches!(ack_kind(MessageOutcome::Ack), AckKind::Ack));
        assert!(matches!(
            ack_kind(MessageOutcome::Nak(None)),
            AckKind::Nak(None)
        ));
        assert!(matches!(
            ack_kind(MessageOutcome::Nak(Some(Duration::from_secs(2)))),
            AckKind::Nak(Some(_))
        ));
        assert!(matches!(ack_kind(MessageOutcome::Term), AckKind::Term));
    }
}
