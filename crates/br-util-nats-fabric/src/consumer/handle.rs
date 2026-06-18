use std::time::Duration;

use async_nats::jetstream::AckKind;
use async_nats::jetstream::Message;
use serde::de::DeserializeOwned;

use crate::error::{ConsumeErrorKind, FabricError};

pub struct Delivered<E> {
    message: Message,
    subject: String,
    delivered_count: i64,
    payload: Result<E, FabricError>,
}

impl<E: DeserializeOwned> Delivered<E> {
    pub(crate) fn decode(message: Message) -> Self {
        let subject = message.subject.as_str().to_string();
        let delivered_count = message.info().map(|info| info.delivered).unwrap_or(1);
        let payload = serde_json::from_slice::<E>(&message.payload)
            .map_err(|err| FabricError::decode(subject.clone(), &err));
        Self {
            message,
            subject,
            delivered_count,
            payload,
        }
    }
}

impl<E> Delivered<E> {
    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn delivered_count(&self) -> i64 {
        self.delivered_count
    }

    pub fn payload(&self) -> Result<&E, &FabricError> {
        self.payload.as_ref()
    }

    pub async fn ack(self) -> Result<(), FabricError> {
        self.apply(AckKind::Ack).await
    }

    pub async fn nak(self, delay: Option<Duration>) -> Result<(), FabricError> {
        self.apply(AckKind::Nak(delay)).await
    }

    pub async fn term(self) -> Result<(), FabricError> {
        self.apply(AckKind::Term).await
    }

    async fn apply(self, kind: AckKind) -> Result<(), FabricError> {
        self.message
            .ack_with(kind)
            .await
            .map_err(|err| FabricError::consume(ConsumeErrorKind::Other, err.to_string()))
    }
}
