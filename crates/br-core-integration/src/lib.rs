#![doc = include_str!("../README.md")]

mod coords;
mod envelopes;
pub mod outbox;
mod outcome;

pub use br_core_events::{Actor, ServiceAccountId, UserId};
pub use coords::{Aggregate, Bc, CommandCoords, CoordError, EventCoords, PastFact, Verb};
pub use envelopes::{EventMetadata, IntegrationCommand, IntegrationEvent};
pub use outbox::{
    OutboxStatus, RETRY_BACKOFF_BASE, RETRY_BACKOFF_MAX, Transition, UnknownOutboxStatus,
    next_after_attempt, retry_backoff,
};
pub use outcome::MessageOutcome;
