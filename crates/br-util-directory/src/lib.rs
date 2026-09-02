#![doc = include_str!("../README.md")]

mod error;
#[cfg(feature = "consumer")]
mod impact;
#[cfg(any(feature = "publisher", feature = "consumer"))]
mod keys;

#[cfg(feature = "consumer")]
mod consumer;
#[cfg(feature = "publisher")]
mod publisher;

pub use error::DirectoryError;

#[cfg(feature = "consumer")]
pub use consumer::{
    ConsumptionScope, DirectoryConsumerConfig, DirectoryProjector, DirectorySnapshot,
    KnownServiceAccount, KnownUser, ManifestState, MemberRow, PersistedExtensions,
    ProjectorProgress, ProjectorProgressReceiver, connect_pool, member_rows, migrate,
};
#[cfg(feature = "consumer")]
pub use impact::{
    ForeignRef, GROUP_NAMESPACE, Impact, ImpactStager, SERVICE_ACCOUNT_NAMESPACE, USER_NAMESPACE,
};
#[cfg(feature = "publisher")]
pub use publisher::{DirectoryPublisher, DirectorySource};
