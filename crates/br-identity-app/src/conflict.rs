//! The save path's **conflict vocabulary**: the [`SaveOutcome`] a
//! [`save`](crate::ScopeRegistryRepository::save) attempt resolves to, and the
//! SQLSTATE classification that tells a `UNIQUE(scope_key)` violation apart from
//! a real persistence fault.
//!
//! The registry enforces its uniqueness invariant at two layers: the aggregate
//! refuses a cross-owner claim in-memory, and a global `UNIQUE(scope_key)` index
//! is the final database net. When the net fires it is **classified here**, not
//! raised as an error — a nak would redeliver, re-violate, and loop forever.

/// Postgres-specific SQLSTATE for a unique-violation (`23505`). The
/// `UNIQUE(scope_key)` net raises it when a row for a key owned elsewhere is
/// inserted.
const UNIQUE_VIOLATION: &str = "23505";

/// The result of a [`save`](crate::ScopeRegistryRepository::save) attempt.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future outcome is an
/// additive change.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveOutcome {
    /// The new state was persisted; the head version advanced to the
    /// aggregate's version.
    Persisted,
    /// The head version moved under us (a concurrent writer won). The pipeline
    /// re-hydrates and re-judges. Benign — never surfaced to a declarant.
    VersionConflict,
    /// A `UNIQUE(scope_key)` violation: the contested key is already owned by
    /// another service. A settled, terminal verdict — the pipeline replies
    /// `rejected(ScopeOwnedByAnotherService)`, never naks. Carries the contested
    /// key **and the actual owning service** (read back from the committed
    /// winner's row) so the rejection names the truth, not the losing declarant.
    ScopeConflict {
        /// The scope key the unique index refused.
        scope_key: String,
        /// The service that actually owns the contested key (the winner).
        owner: String,
    },
}

/// Classify a `sqlx::Error` as a `UNIQUE(scope_key)` violation, returning the
/// contested key when it is one. Anything else returns `None` (the caller
/// treats it as a real persistence fault).
pub(crate) fn classify_unique_violation(err: &sqlx::Error, scope_key: &str) -> Option<String> {
    match err {
        sqlx::Error::Database(db) if db.code().as_deref() == Some(UNIQUE_VIOLATION) => {
            Some(scope_key.to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // classify_unique_violation only fires on SQLSTATE 23505; a generic sqlx
    // error is not a scope conflict (it is a real persistence fault).
    #[test]
    fn classify_unique_violation_ignores_non_db_errors() {
        let err = sqlx::Error::PoolClosed;
        assert_eq!(classify_unique_violation(&err, "notifier:read"), None);
    }
}
