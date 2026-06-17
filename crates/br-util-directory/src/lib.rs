#![doc = include_str!("../README.md")]

mod error;
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
    KnownServiceAccount, KnownUser, ManifestState, MemberRow, PersistedExtensions, connect_pool,
    member_rows, migrate,
};
#[cfg(feature = "publisher")]
pub use publisher::{DirectoryPublisher, DirectorySource};
