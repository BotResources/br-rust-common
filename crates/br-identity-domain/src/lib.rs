#![doc = include_str!("../README.md")]
//!
//! ---
//!
//! # API notes
//!
//! These are the rustdoc-level details the README leaves to the reference: the
//! intra-doc links into the model, and the one rationale the prose does not
//! spell out.
//!
//! ## The model, at a glance
//!
//! - [`ScopeRegistry`] — the **single aggregate**; the command
//!   [`register_declaration`](ScopeRegistry::register_declaration) judges the
//!   registry invariants and returns granular [`RegistryEvent`]s in a
//!   [`CommandResult`]; [`hydrate`](ScopeRegistry::hydrate) is the read-side half
//!   of the double barrier, failing with a [`RegistryHydrationError`].
//! - [`RegisteredService`] — the per-service child entity (manifest + owned
//!   scopes).
//! - [`judge_declaration`] — the **pure receiver-side decision function**,
//!   composing `br-core-scope`'s boundary validation with the aggregate command
//!   into one call the application layer makes between `load` and `save`.
//!
//! ## Why this crate does not lower events to an envelope
//!
//! It emits **domain events** ([`RegistryEvent`], internal to the BC, unprefixed)
//! and deliberately does **not** convert them to the generic
//! `br-core-events::RawEvent`/`DomainEvent` envelope: the registry is a singleton
//! with no natural per-aggregate `Uuid`, and the envelope id / metadata /
//! `aggregate_id` are supplied at persistence time — so that lowering belongs to
//! the application layer, which owns those concerns. Keeping it out here keeps the
//! domain dependency-minimal (`br-core-scope` only) and pure. Likewise the
//! integration-bus accepted/rejected confirmations
//! ([`ServiceScopesAccepted`](br_core_scope::ServiceScopesAccepted) /
//! [`ServiceScopesRejected`](br_core_scope::ServiceScopesRejected)) are the
//! application layer's reply, **not** domain events of this crate.

mod error;
mod event;
mod handler;
mod registry;
mod service;

pub use br_core_scope::{ScopeDeclaration, ScopeDeclarationError};

pub use error::RegistryHydrationError;
pub use event::{CommandResult, RegistryEvent, RegistryWarning};
pub use handler::{DeclarationOutcome, judge_declaration};
pub use registry::ScopeRegistry;
pub use service::RegisteredService;
