#![doc = include_str!("../README.md")]

pub mod awaiter;
pub mod consumer;
mod envelopes;
mod error;
mod nats;
mod nats_classify;
mod noop;
pub mod outbox;
mod outcome;
mod publisher;
mod subject;

pub use awaiter::{CorrelatedAwaiter, CorrelatedMatch};
pub use br_core_events::{Actor, ServiceAccountId, UserId};
pub use consumer::{Delivery, DurableConsumer};
pub use envelopes::{EventMetadata, IntegrationCommand, IntegrationEvent};
pub use error::{ConsumeErrorKind, IntegrationError, PublishErrorKind};
pub use nats::NatsIntegrationPublisher;
pub use noop::NoopIntegrationPublisher;
#[cfg(feature = "outbox")]
pub use outbox::{
    DEFAULT_TABLE, OutboxRelay, OutboxStore, OutboxStoreError, REASON_NO_STREAM, RelayHealth,
    RelayHealthReceiver, RelayPolicy, RelayReport, RelayRunError, stage, stage_into,
};
pub use outbox::{
    FailureClass, OutboxRecord, OutboxStatus, Transition, UnknownOutboxStatus, classify_failure,
    next_after_attempt, retry_backoff, verify_consumer,
};
pub use outcome::MessageOutcome;
pub use publisher::{IntegrationPublisher, IntegrationPublisherExt};
pub use subject::{MessageKind, SubjectError, integration_subject};
