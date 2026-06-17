mod record;

pub use record::OutboxRecord;

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
pub use driver::RelayRunError;
#[cfg(feature = "outbox")]
pub use health::{REASON_NO_STREAM, RelayHealth, RelayHealthReceiver};
#[cfg(feature = "outbox")]
pub use relay::OutboxRelay;
#[cfg(feature = "outbox")]
pub use report::{
    DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_MESSAGES, FailureClass, RelayPolicy, RelayReport,
    classify_failure,
};
#[cfg(feature = "outbox")]
pub use stage::stage;
#[cfg(feature = "outbox")]
pub use store::{
    OUTBOX_NOTIFY_CHANNEL, OUTBOX_TABLE, OutboxStore, OutboxStoreError, PendingOutbox,
};
