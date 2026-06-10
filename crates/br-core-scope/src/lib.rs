//! Pure contract types for the **scope self-declaration** handshake.
//!
//! This crate is the **shared language** both sides of the handshake depend on:
//! a declaring service publishes its scopes, and Identity accepts or rejects
//! them. It is **pure** — no I/O, no transport, no `async` — so the same types
//! are used to *build* a declaration locally (with validation) and to *carry*
//! it over the bus.
//!
//! ## The handshake (epic context)
//!
//! 1. A service builds a *validated* [`ScopeDeclaration`] (its [`ServiceManifest`]
//!    and the [`ScopeSpec`]s it owns) and wraps it via
//!    [`DeclareServiceScopes::new`], published as the `T` of an
//!    `IntegrationCommand` on `identity.cmd.service_scope.declare.v1` — on the
//!    wire the declaration travels in its raw ([`RawScopeDeclaration`]) form.
//! 2. Identity deserializes the [`DeclareServiceScopes`] payload (always possible
//!    when the JSON is structurally well-formed) and calls
//!    [`DeclareServiceScopes::validate`], then validates against its registry and
//!    replies with either a [`ServiceScopesAccepted`] or a
//!    [`ServiceScopesRejected`] payload (the `T` of an `IntegrationEvent`) on
//!    `identity.evt.service_scope.{accepted,rejected}.v1` — it **never**
//!    nak/poison-terms a declaration it can structurally read, answering an
//!    invalid one with a correlated, structured rejection.
//!
//! Correlation, causation, and timestamps live on the envelope's
//! `MessageMetadata` (in `br-core-integration`), **never** in these payloads.
//! The envelopes are generic over `T`, so this crate carries **no dependency**
//! on `br-core-integration` (nor on any `util` crate): tier `core`. The
//! integration envelope is composed at the consumer — there is no dependency in
//! this direction.
//!
//! ## Validation, and where it lives
//!
//! - **Intrinsic** key syntax is enforced in the constructors ([`ScopeKey::new`],
//!   [`ServiceKey::new`]). Deserializing a *bare* [`ScopeKey`] / [`ServiceKey`] /
//!   [`ScopeSpec`] / [`ScopeDeclaration`] re-runs that validation and **fails
//!   closed with an opaque `serde` error** — a deliberate property: those are the
//!   validated types, so a bad wire value must not reconstruct one.
//! - The **contextual** rule "a scope's `{service}` segment must equal the
//!   declaring service's key" needs the declarant, so it is *not* in the key
//!   constructor; it is enforced when a [`ScopeDeclaration`] is assembled (and
//!   surfaces as [`ScopeDeclarationError::ScopePrefixMismatch`]).
//! - The **structured** [`ScopeDeclarationError::InvalidScopeKey`] (the offending
//!   key + the precise [`KeyValidationError`]) is produced by the *explicit*
//!   validation of the receiver-side raw form
//!   ([`RawScopeDeclaration::validate`] / [`DeclareServiceScopes::validate`]) —
//!   the path the protocol uses so a malformed key yields a structured rejection,
//!   not an unreadable nak — and by Identity's registry.
//! - There is intentionally **no anti-spoof / `ServiceIdentityMismatch`** check:
//!   the bus is auth-less behind a default-deny NetworkPolicy, so there is no
//!   authenticated principal to bind a declaration to — shipping such a check
//!   would be a false guarantee.
//!
//! ## Rejection language
//!
//! [`ScopeDeclarationError`] is one shared enum.
//! [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey),
//! [`ScopePrefixMismatch`](ScopeDeclarationError::ScopePrefixMismatch) and
//! [`DuplicateScopeInDeclaration`](ScopeDeclarationError::DuplicateScopeInDeclaration)
//! are produced by the explicit validation of the raw form (the receiver path)
//! and by Identity's registry; the cross-cutting two are also produced by the
//! local [`ScopeDeclaration::new`]. The remaining
//! [`ScopeOwnedByAnotherService`](ScopeDeclarationError::ScopeOwnedByAnotherService)
//! is produced **only** by Identity's registry. It is the payload of
//! [`ServiceScopesRejected`], so it (de)serializes. Per codes-not-language, its
//! `Display` strings are stable codes, never UI prose.

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
