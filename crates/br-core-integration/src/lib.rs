#![doc = include_str!("../README.md")]

pub mod awaiter;
mod awaiter_config;
pub mod consumer;
mod envelopes;
mod error;
mod nats;
mod nats_classify;
mod noop;
mod outcome;
mod publisher;
mod subject;

pub use awaiter::{CorrelatedAwaiter, CorrelatedMatch};
pub use awaiter_config::AwaiterConfig;
pub use br_core_events::{Actor, ServiceAccountId, UserId};
pub use consumer::{Delivery, DurableConsumer};
pub use envelopes::{IntegrationCommand, IntegrationEvent, MessageMetadata};
pub use error::{ConsumeErrorKind, IntegrationError, PublishErrorKind};
pub use nats::NatsIntegrationPublisher;
pub use noop::NoopIntegrationPublisher;
pub use outcome::MessageOutcome;
pub use publisher::{IntegrationPublisher, IntegrationPublisherExt};
pub use subject::{MessageKind, SubjectError, integration_subject};
