mod record;
mod retry;
mod status;
mod verify;

pub use record::OutboxRecord;
pub use retry::{
    FailureClass, RETRY_BACKOFF_BASE, RETRY_BACKOFF_MAX, classify_failure, retry_backoff,
};
pub use status::{OutboxStatus, Transition, UnknownOutboxStatus, next_after_attempt};
pub use verify::verify_consumer;

#[cfg(feature = "outbox")]
mod driver;
#[cfg(feature = "outbox")]
mod health;
#[cfg(feature = "outbox")]
mod relay;
#[cfg(feature = "outbox")]
mod report;
#[cfg(feature = "outbox")]
mod stage;
#[cfg(feature = "outbox")]
mod store;
#[cfg(feature = "outbox")]
mod table_name;

#[cfg(feature = "outbox")]
pub use driver::RelayRunError;
#[cfg(feature = "outbox")]
pub use health::{REASON_NO_STREAM, RelayHealth, RelayHealthReceiver};
#[cfg(feature = "outbox")]
pub use relay::OutboxRelay;
#[cfg(feature = "outbox")]
pub use report::{DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_MESSAGES, RelayPolicy, RelayReport};
#[cfg(feature = "outbox")]
pub use stage::{stage, stage_into};
#[cfg(feature = "outbox")]
pub use store::{DEFAULT_TABLE, OutboxStore, OutboxStoreError};
