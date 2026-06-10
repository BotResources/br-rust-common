#![doc = include_str!("../README.md")]
//!
//! ---
//!
//! # API notes
//!
//! The rustdoc-level details the README leaves to the reference: the public
//! surface and the layering.
//!
//! ## The layering, at a glance
//!
//! - [`migrate`] — apply the embedded `scope_registry` schema, explicitly (never
//!   auto-provisioning).
//! - [`ScopeRegistryRepository`] — the Postgres adapter: [`load`] (hydrate the
//!   aggregate + the optimistic-lock version) and [`save`] (version CAS, with the
//!   `UNIQUE(scope_key)` violation classified into a [`SaveOutcome::ScopeConflict`]
//!   rather than raised as an error).
//! - [`ConfirmationPublisher`] — emits the correlated `accepted` / `rejected`
//!   `IntegrationEvent`s on the integration bus.
//! - [`ScopeDeclarationPipeline`] — the uniform `load → judge → save → dispatch`,
//!   the bounded optimistic-lock retry, and the conflict mapping. Holds **no
//!   business logic** — every verdict is the domain's
//!   [`judge_declaration`](br_identity_domain::judge_declaration).
//! - [`run_scope_declarations`] — binds the pre-declared durable consumer and
//!   drives the pipeline, mapping the outcome to a JetStream ack.
//!
//! ## Why this crate, not the domain, lowers events to envelopes
//!
//! The domain deliberately does not convert its [`RegistryEvent`]s to a generic
//! envelope (no per-aggregate id for a singleton; metadata supplied at
//! persistence time). This slice does not need a domain-bus fan-out, so it
//! **logs** the events rather than publishing them — an honest, documented
//! no-pretend-dispatch decision (see the README). The integration-bus
//! accepted/rejected confirmations are this crate's reply and live in
//! [`ConfirmationPublisher`].
//!
//! [`load`]: ScopeRegistryRepository::load
//! [`save`]: ScopeRegistryRepository::save
//! [`RegistryEvent`]: br_identity_domain::RegistryEvent

mod conflict;
mod consumer;
mod error;
mod hydration;
mod migrations;
mod pipeline;
mod publisher;
mod repository;

pub use conflict::SaveOutcome;
pub use consumer::run_scope_declarations;
pub use error::AppError;
pub use migrations::migrate;
pub use pipeline::{HandledOutcome, ScopeDeclarationPipeline};
pub use publisher::ConfirmationPublisher;
pub use repository::ScopeRegistryRepository;
