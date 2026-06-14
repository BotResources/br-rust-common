#![doc = include_str!("../README.md")]

mod error;

#[cfg(feature = "consumer")]
mod consumer;
#[cfg(feature = "publisher")]
mod publisher;

pub use error::DirectoryError;

#[cfg(feature = "consumer")]
pub use consumer::{
    DirectoryProjector, DirectorySnapshot, KnownUser, MemberRow, connect_pool, member_rows, migrate,
};
#[cfg(feature = "publisher")]
pub use publisher::{DirectoryPublisher, DirectorySource, KvOp, reconcile_entries};
