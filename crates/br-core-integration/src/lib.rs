#![doc = include_str!("../README.md")]
//!
//! ---
//!
//! # API notes
//!
//! The rustdoc cross-links the README leaves to the reference.
//!
//! - **Envelopes & metadata:** [`IntegrationEvent<T>`] (fact) and
//!   [`IntegrationCommand<T>`] (request), both `#[non_exhaustive]` — build via
//!   their `new`. [`MessageMetadata`] is `br_core_events::EventMetadata`
//!   re-exported (one type, one wire contract); construct it from this crate
//!   alone via the re-exported [`Actor`] / [`UserId`] / [`ServiceAccountId`].
//! - **Publishing:** [`IntegrationPublisher`] is object-safe (hold an
//!   `Arc<dyn IntegrationPublisher>`); typed helpers
//!   ([`publish_event`](IntegrationPublisherExt::publish_event),
//!   [`publish_command`](IntegrationPublisherExt::publish_command), and the
//!   `_if_connected` variants) come from the [`IntegrationPublisherExt`] blanket.
//!   [`NatsIntegrationPublisher`] awaits the broker ack; [`NoopIntegrationPublisher`]
//!   is for tests. A failed publish is [`IntegrationError::Publish`] carrying a
//!   classified [`PublishErrorKind`].
//! - **Consuming:** the **receiver** shape is [`DurableConsumer`] (+ [`Delivery`],
//!   [`MessageOutcome`]); the **awaiter** shape is [`CorrelatedAwaiter`] (+
//!   [`CorrelatedMatch`], [`AwaiterConfig`]). A bind failure is
//!   [`IntegrationError::Consume`] carrying a [`ConsumeErrorKind`].
//! - **Subjects:** [`integration_subject`] (+ [`MessageKind`], [`SubjectError`])
//!   is the single source of the `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}`
//!   convention.
//! - **Outbox:** [`outbox`] is the transactional outbox — stage a message in the
//!   caller's transaction ([`OutboxRecord`], `stage`) and publish it post-commit
//!   with the crash-recovery `OutboxRelay` (both behind the `outbox` feature; the
//!   [`OutboxStatus`] state machine is always available). See the
//!   [module docs](outbox) for the at-least-once, post-commit semantics and the
//!   declared-table contract.

pub mod awaiter;
mod awaiter_config;
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

// Re-exported so consumers can construct `MessageMetadata` (whose `new` takes
// an `Actor`) from this crate alone, without adding a kernel dependency.
pub use awaiter::{CorrelatedAwaiter, CorrelatedMatch};
pub use awaiter_config::AwaiterConfig;
pub use br_core_events::{Actor, ServiceAccountId, UserId};
pub use consumer::{Delivery, DurableConsumer};
pub use envelopes::{IntegrationCommand, IntegrationEvent, MessageMetadata};
pub use error::{ConsumeErrorKind, IntegrationError, PublishErrorKind};
pub use nats::NatsIntegrationPublisher;
pub use noop::NoopIntegrationPublisher;
#[cfg(feature = "outbox")]
pub use outbox::{
    DEFAULT_TABLE, OutboxRelay, OutboxStore, OutboxStoreError, RelayPolicy, RelayReport, stage,
    stage_into,
};
pub use outbox::{
    OutboxRecord, OutboxStatus, Transition, UnknownOutboxStatus, next_after_attempt,
    verify_consumer,
};
pub use outcome::MessageOutcome;
pub use publisher::{IntegrationPublisher, IntegrationPublisherExt};
pub use subject::{MessageKind, SubjectError, integration_subject};
