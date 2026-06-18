use async_nats::jetstream::consumer::PullConsumer;
use async_nats::jetstream::consumer::pull::Stream as PullStream;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use br_core_integration::{IntegrationCommand, IntegrationEvent};

use crate::classify::classify_messages_error;
use crate::consumer::handle::Delivered;
use crate::error::{ConsumeErrorKind, FabricError};

pub type CommandConsumer<T> = IntegrationConsumer<IntegrationCommand<T>>;
pub type EventConsumer<T> = IntegrationConsumer<IntegrationEvent<T>>;

pub struct IntegrationConsumer<E> {
    messages: PullStream,
    _marker: std::marker::PhantomData<fn() -> E>,
}

impl<E: DeserializeOwned> IntegrationConsumer<E> {
    pub(crate) async fn open_stream(consumer: PullConsumer) -> Result<Self, FabricError> {
        let messages = consumer
            .messages()
            .await
            .map_err(|e| FabricError::consume(ConsumeErrorKind::Other, e.to_string()))?;
        Ok(Self {
            messages,
            _marker: std::marker::PhantomData,
        })
    }

    pub async fn recv(&mut self) -> Result<Option<Delivered<E>>, FabricError> {
        match self.messages.next().await {
            None => Ok(None),
            Some(Ok(message)) => Ok(Some(Delivered::decode(message))),
            Some(Err(err)) => Err(FabricError::consume(
                classify_messages_error(&err),
                err.to_string(),
            )),
        }
    }
}
