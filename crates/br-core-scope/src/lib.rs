#![doc = include_str!("../README.md")]
//!
//! ---
//!
//! # API notes
//!
//! The rustdoc-level cross-links the README leaves to the reference.
//!
//! - **Validated newtypes:** [`ScopeKey`] (with [`SCOPE_KEY_MAX_LEN`]) and
//!   [`ServiceKey`] (with [`SERVICE_KEY_MAX_LEN`]) — intrinsic syntax in the
//!   constructor, re-run on deserialize (fail closed). The contextual
//!   "a scope's `{service}` segment must equal the declaring service" rule lives
//!   in [`ScopeKey::is_owned_by`], not the constructor.
//! - **Declaration shapes:** [`ScopeSpec`] / [`ServiceManifest`] build a validated
//!   [`ScopeDeclaration`] ([`ScopeDeclaration::new`] validates atomically); the
//!   receiver-side raw form is [`RawScopeDeclaration`] / [`RawScopeSpec`] /
//!   [`RawServiceManifest`], whose [`validate`](RawScopeDeclaration::validate) is
//!   the sole path back to a validated declaration.
//! - **Messages:** [`DeclareServiceScopes`] (the command payload, carrying the
//!   declaration in raw form), [`ServiceScopesAccepted`], [`ServiceScopesRejected`].
//! - **Rejection language:** [`ScopeDeclarationError`] (the shared, (de)serializable
//!   reason enum) and the [`KeyValidationError`] it can carry.

mod declaration;
mod error;
mod key;
mod messages;
mod raw;
mod service;
mod spec;

pub use declaration::ScopeDeclaration;
pub use error::{KeyValidationError, ScopeDeclarationError};
pub use key::{SCOPE_KEY_MAX_LEN, ScopeKey};
pub use messages::{DeclareServiceScopes, ServiceScopesAccepted, ServiceScopesRejected};
pub use raw::{RawScopeDeclaration, RawScopeSpec, RawServiceManifest};
pub use service::{SERVICE_KEY_MAX_LEN, ServiceKey};
pub use spec::{ScopeSpec, ServiceManifest};
