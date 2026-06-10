//! Shared data structures for domain events.
//!
//! [`RawEvent`] is what aggregates emit before persistence. [`DomainEvent`] is
//! what the event store stores and replays. [`EventMetadata`] carries the
//! identity/correlation context attached to each event.
//!
//! ## Construction
//!
//! All three types are `#[non_exhaustive]`, so struct-literal construction is
//! no longer possible from outside the crate — build them through their
//! constructors ([`EventMetadata::new`] / [`EventMetadata::with_causation`],
//! [`RawEvent::new`], [`DomainEvent::new`]). Fields stay `pub` for read access;
//! note that `#[non_exhaustive]` also requires cross-crate pattern matches to
//! include a `..` rest pattern.
//!
//! [`Actor`] (and the [`UserId`] / [`ServiceAccountId`] it wraps) is
//! re-exported from `br-core-kernel` so consumers can construct metadata
//! without adding a direct kernel dependency.
//!
//! ## Actor & wire compatibility
//!
//! As of 0.4.0, [`EventMetadata`] carries a typed [`Actor`]
//! (human or machine) instead of a bare `actor_id: Uuid`. The **wire format is
//! backward-compatible**: it serializes flat `actor_id` + `actor_kind`, and a
//! pre-0.4.0 payload (no `actor_kind`) deserializes to a human actor. The full
//! contract is documented on [`EventMetadata`].

mod events;
mod metadata;

pub use br_core_kernel::{Actor, ServiceAccountId, UserId};
pub use events::{DomainEvent, RawEvent};
pub use metadata::EventMetadata;
