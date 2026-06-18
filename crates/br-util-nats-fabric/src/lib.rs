#![doc = include_str!("../README.md")]

mod awaiter;
mod classify;
mod consumer;
mod coords;
mod error;
mod fabric;
mod kv;
mod outbox;
mod stream;

pub use awaiter::{CorrelatedAwaiter, CorrelatedMatch};
pub use consumer::{CommandConsumer, Delivered, Delivery, EventConsumer, IntegrationConsumer};
pub use coords::{
    Aggregate, Bc, CommandCoords, CoordError, EventCoords, EventSubjectParseError, PastFact, Verb,
    command_subject, event_subject, parse_event_subject,
};
pub use error::{ConsumeErrorKind, FabricError, PublishErrorKind};
pub use fabric::Fabric;
pub use kv::{
    EphemeralAuthStore, KV_EPHEMERAL_AUTH, KV_PUBLISHED_LANGUAGE, KvKey, KvKeyError, KvOp,
    KvPrefix, ProjectionError, ProjectionSink, PublishedLanguageConsumer,
    PublishedLanguagePublisher, PublishedLanguageReader, Revision, WatchHealth,
    WatchHealthReceiver, reconcile,
};
pub use outbox::OutboxRecord;
pub use stream::{INTEGRATION_CMD, INTEGRATION_EVT};

#[cfg(feature = "outbox")]
pub use outbox::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_MESSAGES, FailureClass, OUTBOX_NOTIFY_CHANNEL, OUTBOX_TABLE,
    OutboxRelay, OutboxStore, OutboxStoreError, PendingOutbox, REASON_NO_STREAM, RelayHealth,
    RelayHealthReceiver, RelayPolicy, RelayReport, RelayRunError, classify_failure, stage,
};

pub use br_core_events::EventMetadata;
pub use br_core_integration::{IntegrationCommand, IntegrationEvent, MessageOutcome};
