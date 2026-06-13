const UNIQUE_VIOLATION: &str = "23505";

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SaveOutcome {
    Persisted,
    VersionConflict,
    ScopeConflict { scope_key: String, owner: String },
}

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

    #[test]
    fn classify_unique_violation_ignores_non_db_errors() {
        let err = sqlx::Error::PoolClosed;
        assert_eq!(classify_unique_violation(&err, "notifier:read"), None);
    }
}
