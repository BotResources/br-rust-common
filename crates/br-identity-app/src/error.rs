//! [`AppError`] — this crate's own error type.
//!
//! It is the application/adapter layer's error: persistence faults, transport
//! faults, and a corrupt persisted state that fails the domain's hydration
//! barrier. It is deliberately **distinct** from the domain's rejection
//! language ([`ScopeDeclarationError`](br_core_scope::ScopeDeclarationError)): a
//! *rejection* is a normal, expected verdict on a declaration (it is published
//! as a `rejected` confirmation, never an error here), whereas an `AppError` is
//! an infrastructure failure or a fail-loud corruption signal that aborts the
//! pipeline run.
//!
//! Per the platform rule that a crate never leaks a lower layer's error across
//! its public API, the `sqlx` / integration / hydration sources are wrapped, not
//! re-exported.

use br_identity_domain::RegistryHydrationError;

/// Why a pipeline step failed at the application/adapter layer.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future variant is an
/// additive change.
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum AppError {
    /// A persistence operation failed (connect, query, transaction). Carries the
    /// underlying `sqlx` error for logs; it is never surfaced to a declarant.
    #[error("registry persistence failed: {0}")]
    Persistence(#[from] sqlx::Error),

    /// Loading the registry produced a state the domain considers impossible —
    /// the read-side half of the double barrier tripped. A corrupt persisted
    /// state fails loud here rather than serving an illegal registry.
    #[error("persisted registry state failed hydration: {0}")]
    Hydration(#[from] RegistryHydrationError),

    /// Publishing a confirmation onto the integration bus failed at the
    /// transport. Carries the integration error for logs.
    #[error("confirmation publish failed: {0}")]
    Publish(#[from] br_core_integration::IntegrationError),

    /// The optimistic-lock head row was missing when the save path expected it
    /// — the declared `scope_registry_head` singleton row is absent, which is a
    /// migration/bootstrap fault (the migration seeds it). Fail loud rather than
    /// silently re-create it (the lib never auto-provisions).
    #[error("scope_registry_head row is missing; run migrations")]
    MissingRegistryHead,

    /// The save path exhausted its bounded optimistic-lock retries: every
    /// attempt re-hydrated, re-judged, and lost the version CAS to a concurrent
    /// writer. Truly exceptional under the mono-pod write model; surfaced so the
    /// handler can nak-with-delay for a later redelivery.
    #[error("optimistic-lock conflict persisted after {attempts} retries")]
    ConflictRetriesExhausted {
        /// How many attempts were made before giving up.
        attempts: u32,
    },

    /// A persisted key string failed re-validation while loading — the store
    /// holds a value the validated types ([`ScopeKey`](br_core_scope::ScopeKey)
    /// / [`ServiceKey`](br_core_scope::ServiceKey)) reject. This is the
    /// read-side barrier for *key-syntax* corruption (distinct from the domain's
    /// cross-row [`Hydration`](AppError::Hydration) inconsistencies): a malformed
    /// stored key is a store fault that fails loud rather than reconstructing a
    /// type that can never be valid.
    #[error("persisted key {value:?} failed re-validation: corrupt store")]
    CorruptStoredKey {
        /// The malformed stored value.
        value: String,
    },
}

impl AppError {
    /// Whether this failure is **permanent** — it cannot heal by redelivering the
    /// same command, only by an operator repairing the store.
    ///
    /// The two read-side double-barrier trips are permanent: a registry that
    /// fails [`Hydration`](AppError::Hydration) (a cross-row inconsistency the
    /// domain considers impossible) or holds a [`CorruptStoredKey`] (a value the
    /// validated types reject) is corrupt at rest — every redelivery re-loads the
    /// same corrupt rows and re-fails identically until the rows are fixed.
    ///
    /// Everything else is **transient**: a [`Persistence`](AppError::Persistence)
    /// or [`Publish`](AppError::Publish) fault is a DB/transport blip that the
    /// next redelivery may clear, [`ConflictRetriesExhausted`] is a contention
    /// burst that subsides, and [`MissingRegistryHead`](AppError::MissingRegistryHead)
    /// is resolved by running the migration that seeds the row — after which a
    /// redelivery succeeds. The consumer naks **both** classes (the redelivered
    /// command must succeed once the store is repaired, with no restart); the
    /// distinction drives the *signal* — a permanent fault logs loudly and fires
    /// the operator-remediation callback, a transient one logs at `warn`.
    ///
    /// [`CorruptStoredKey`]: AppError::CorruptStoredKey
    /// [`ConflictRetriesExhausted`]: AppError::ConflictRetriesExhausted
    #[must_use]
    pub fn is_permanent(&self) -> bool {
        matches!(
            self,
            AppError::Hydration(_) | AppError::CorruptStoredKey { .. }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_identity_domain::RegistryHydrationError;

    // A hydration-barrier trip is permanent: redelivery re-loads the same corrupt
    // rows and re-fails — only an operator PG fix clears it.
    #[test]
    fn hydration_failure_is_permanent() {
        let err = AppError::Hydration(RegistryHydrationError::DuplicateScope {
            key: br_core_scope::ScopeKey::new("notifier:read").unwrap(),
        });
        assert!(err.is_permanent());
    }

    // A corrupt stored key is permanent for the same reason: the stored value
    // never re-parses, no matter how often the command is redelivered.
    #[test]
    fn corrupt_stored_key_is_permanent() {
        let err = AppError::CorruptStoredKey {
            value: "BAD KEY".to_string(),
        };
        assert!(err.is_permanent());
    }

    // A persistence fault is transient: a DB blip the next redelivery may clear.
    #[test]
    fn persistence_failure_is_transient() {
        let err = AppError::Persistence(sqlx::Error::PoolClosed);
        assert!(!err.is_permanent());
    }

    // A publish fault is transient (a transport blip).
    #[test]
    fn publish_failure_is_transient() {
        let err = AppError::Publish(br_core_integration::IntegrationError::Serialization(
            serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
        ));
        assert!(!err.is_permanent());
    }

    // Exhausted optimistic-lock retries are transient (a contention burst).
    #[test]
    fn conflict_retries_exhausted_is_transient() {
        let err = AppError::ConflictRetriesExhausted { attempts: 5 };
        assert!(!err.is_permanent());
    }

    // A missing head row is transient w.r.t. redelivery: running the migration
    // seeds it, after which a redelivered command succeeds with no restart.
    #[test]
    fn missing_registry_head_is_transient() {
        let err = AppError::MissingRegistryHead;
        assert!(!err.is_permanent());
    }
}
