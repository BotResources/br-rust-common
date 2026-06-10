//! [`RegistryHydrationError`] — why a *persisted* [`ScopeRegistry`] state failed
//! to load.
//!
//! This is **this crate's own error type**, distinct from the shared
//! [`ScopeDeclarationError`](br_core_scope::ScopeDeclarationError) rejection
//! language: rejecting a *declaration* (a write the registry refuses) and
//! refusing to *load* a corrupt persisted state are different failures with
//! different audiences. The rejection language is a UX-facing reply to a
//! declarant; a hydration error is an operator-facing signal that the store
//! holds a state the domain considers impossible.
//!
//! It exists because of the **double barrier**: the registry invariants are
//! enforced at command time *and* re-validated when the aggregate is rebuilt
//! from persisted state, so a bad past write or a migration gap can never
//! resurrect an illegal registry — a malformed state fails to load loudly
//! instead of silently serving corruption.
//!
//! Per codes-not-language, the `#[error("…")]` strings are **stable codes**, not
//! UI prose.

use br_core_scope::{ScopeKey, ServiceKey};

/// Why a persisted [`ScopeRegistry`](crate::ScopeRegistry) state is malformed
/// and was refused at hydration.
///
/// Each variant is a *cross-row consistency* break — the individual keys are
/// already validated types, so key *syntax* cannot be wrong here; what the
/// hydration constructor re-checks is that the rows agree with one another and
/// with the same invariants a command would have enforced.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future consistency rule
/// stays an additive change.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryHydrationError {
    /// The same scope key appears twice in the persisted state — the
    /// uniqueness invariant the registry exists to hold was already broken in
    /// the store.
    #[error("duplicate_scope_in_registry")]
    DuplicateScope {
        /// The duplicated key.
        key: ScopeKey,
    },
    /// A persisted scope's `{service}` segment does not match the service it is
    /// recorded as owned by — the ownership/prefix consistency a declaration
    /// always enforces was violated in the store.
    #[error("scope_owner_mismatch")]
    ScopeOwnerMismatch {
        /// The scope key whose prefix disagrees with its recorded owner.
        key: ScopeKey,
        /// The service the row records as owning the scope.
        owning_service: ServiceKey,
    },
}
