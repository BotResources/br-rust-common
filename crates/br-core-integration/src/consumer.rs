use std::future::Future;

use async_nats::jetstream::consumer::PullConsumer;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::nats_classify::{classify_consumer_info, classify_get_stream, classify_messages_error};
use crate::{IntegrationCommand, IntegrationError, IntegrationEvent, MessageOutcome};

#[non_exhaustive]
pub struct Delivery<E> {
    pub subject: String,
    pub envelope: E,
}

pub struct DurableConsumer {
    consumer: PullConsumer,
    stream_name: String,
    consumer_name: String,
}

impl DurableConsumer {
    pub async fn bind(
        jetstream: &async_nats::jetstream::Context,
        stream_name: impl Into<String>,
        consumer_name: impl Into<String>,
    ) -> Result<Self, IntegrationError> {
        let stream_name = stream_name.into();
        let consumer_name = consumer_name.into();

        let stream = jetstream
            .get_stream(&stream_name)
            .await
            .map_err(|e| IntegrationError::consume(classify_get_stream(&e), e.to_string()))?;
        let consumer: PullConsumer = stream.get_consumer(&consumer_name).await.map_err(|e| {
            match e.downcast_ref::<async_nats::jetstream::context::ConsumerInfoError>() {
                Some(info_err) => IntegrationError::consume(
                    classify_consumer_info(info_err),
                    info_err.to_string(),
                ),
                None => IntegrationError::consume(crate::ConsumeErrorKind::Other, e.to_string()),
            }
        })?;

        Ok(Self {
            consumer,
            stream_name,
            consumer_name,
        })
    }

    pub fn stream_name(&self) -> &str {
        &self.stream_name
    }

    pub fn consumer_name(&self) -> &str {
        &self.consumer_name
    }

    pub async fn run_commands<T, H, HFut, P>(
        self,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), IntegrationError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationCommand<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(IntegrationError),
    {
        self.run_inner::<IntegrationCommand<T>, _, _, _>(&mut handler, &mut on_poison)
            .await
    }

    pub async fn run_events<T, H, HFut, P>(
        self,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), IntegrationError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationEvent<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(IntegrationError),
    {
        self.run_inner::<IntegrationEvent<T>, _, _, _>(&mut handler, &mut on_poison)
            .await
    }

    async fn run_inner<E, H, HFut, P>(
        self,
        handler: &mut H,
        on_poison: &mut P,
    ) -> Result<(), IntegrationError>
    where
        E: DeserializeOwned,
        H: FnMut(Delivery<E>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(IntegrationError),
    {
        let mut messages = self.consumer.messages().await.map_err(|e| {
            IntegrationError::consume(crate::ConsumeErrorKind::Other, e.to_string())
        })?;

        while let Some(message) = messages.next().await {
            let message = message.map_err(|e| {
                IntegrationError::consume(classify_messages_error(&e), e.to_string())
            })?;
            let subject = message.subject.as_str().to_string();

            match serde_json::from_slice::<E>(&message.payload) {
                Ok(envelope) => {
                    let outcome = handler(Delivery { subject, envelope }).await;
                    apply_outcome(&message, outcome).await;
                }
                Err(decode_err) => {
                    let _ = message.ack_with(async_nats::jetstream::AckKind::Term).await;
                    on_poison(IntegrationError::decode(subject, &decode_err));
                }
            }
        }

        Ok(())
    }
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
