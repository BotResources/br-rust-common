#![doc = include_str!("../README.md")]
//!
//! ---
//!
//! # API notes
//!
//! The rustdoc cross-links the README leaves to the reference.
//!
//! - [`declare_scopes`] — the boot-time handshake entry point: subscribe-first,
//!   re-publish-on-timeout, and the readiness flips. It drives a
//!   [`CorrelatedAwaiter`](br_core_integration::CorrelatedAwaiter) over the two
//!   confirmation subjects and fails loud (via
//!   [`IntegrationError::Consume`](br_core_integration::IntegrationError)) if the
//!   pre-declared stream is missing — it never creates infrastructure.
//! - [`ScopeDeclarationConfig`] — `enabled(stream)` / `disabled(stream)`: the
//!   per-project opt-out wired from Helm (disabled still sets readiness UP).
//! - [`ScopeDeclarationOutcome`] — `Accepted` / `Disabled` / `Rejected(reason)`,
//!   `#[non_exhaustive]`; match it additively.
//! - [`declaring_actor`] — the deterministic, name-based declaring-service id
//!   (declarative provenance, not authentication).

pub mod actor;
mod config;
mod handshake;
mod outcome;
mod subjects;

pub use actor::declaring_actor;
pub use config::ScopeDeclarationConfig;
pub use handshake::declare_scopes;
pub use outcome::ScopeDeclarationOutcome;
