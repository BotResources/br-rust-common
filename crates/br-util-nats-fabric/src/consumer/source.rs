use async_nats::jetstream::AckKind;
use async_nats::jetstream::consumer::PullConsumer;
use async_nats::jetstream::consumer::pull::Stream as PullStream;
use futures_util::StreamExt;

use crate::classify::classify_messages_error;
use crate::consumer::bind::ensure_durable;
use crate::consumer::config::ConsumerTuning;
use crate::error::{ConsumeErrorKind, FabricError};

#[async_trait::async_trait]
pub(crate) trait MessageSource: Send {
    type Frame: SourceFrame;
    async fn next(&mut self) -> Option<Result<Self::Frame, FabricError>>;
    async fn rebind(&mut self) -> Result<(), FabricError>;
}

#[async_trait::async_trait]
pub(crate) trait SourceFrame: Send {
    fn subject(&self) -> &str;
    fn payload(&self) -> &[u8];
    async fn ack(self, kind: AckKind) -> Result<(), String>;
}

pub(crate) struct PullFrame(async_nats::jetstream::Message);

#[async_trait::async_trait]
impl SourceFrame for PullFrame {
    fn subject(&self) -> &str {
        self.0.subject.as_str()
    }

    fn payload(&self) -> &[u8] {
        self.0.payload.as_ref()
    }

    async fn ack(self, kind: AckKind) -> Result<(), String> {
        self.0.ack_with(kind).await.map_err(|err| err.to_string())
    }
}

pub(crate) struct PullSource {
    messages: PullStream,
    jetstream: async_nats::jetstream::Context,
    stream_name: &'static str,
    durable: String,
    filter: String,
    tuning: ConsumerTuning,
}

impl PullSource {
    pub(crate) async fn open(
        jetstream: async_nats::jetstream::Context,
        consumer: PullConsumer,
        stream_name: &'static str,
        durable: String,
        filter: String,
        tuning: ConsumerTuning,
    ) -> Result<Self, FabricError> {
        let messages = open_messages(&consumer).await?;
        Ok(Self {
            messages,
            jetstream,
            stream_name,
            durable,
            filter,
            tuning,
        })
    }
}

async fn open_messages(consumer: &PullConsumer) -> Result<PullStream, FabricError> {
    consumer
        .messages()
        .await
        .map_err(|e| FabricError::consume(ConsumeErrorKind::Other, e.to_string()))
}

#[async_trait::async_trait]
impl MessageSource for PullSource {
    type Frame = PullFrame;

    async fn next(&mut self) -> Option<Result<Self::Frame, FabricError>> {
        match self.messages.next().await {
            None => None,
            Some(Ok(message)) => Some(Ok(PullFrame(message))),
            Some(Err(err)) => Some(Err(FabricError::consume(
                classify_messages_error(&err),
                err.to_string(),
            ))),
        }
    }

    async fn rebind(&mut self) -> Result<(), FabricError> {
        let consumer = ensure_durable(
            &self.jetstream,
            self.stream_name,
            &self.durable,
            &self.filter,
            &self.tuning,
        )
        .await?;
        self.messages = open_messages(&consumer).await?;
        Ok(())
    }
}
