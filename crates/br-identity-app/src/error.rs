use br_identity_domain::RegistryHydrationError;

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum AppError {
    #[error("registry persistence failed: {0}")]
    Persistence(#[from] sqlx::Error),

    #[error("persisted registry state failed hydration: {0}")]
    Hydration(#[from] RegistryHydrationError),

    #[error("confirmation publish failed: {0}")]
    Publish(#[from] br_util_nats_fabric::FabricError),

    #[error("scope_registry_head row is missing; run migrations")]
    MissingRegistryHead,

    #[error("optimistic-lock conflict persisted after {attempts} retries")]
    ConflictRetriesExhausted { attempts: u32 },

    #[error("persisted key {value:?} failed re-validation: corrupt store")]
    CorruptStoredKey { value: String },
}

impl AppError {
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

    #[test]
    fn hydration_failure_is_permanent() {
        let err = AppError::Hydration(RegistryHydrationError::DuplicateScope {
            key: br_core_scope::ScopeKey::new("notifier:read").unwrap(),
        });
        assert!(err.is_permanent());
    }

    #[test]
    fn corrupt_stored_key_is_permanent() {
        let err = AppError::CorruptStoredKey {
            value: "BAD KEY".to_string(),
        };
        assert!(err.is_permanent());
    }

    #[test]
    fn persistence_failure_is_transient() {
        let err = AppError::Persistence(sqlx::Error::PoolClosed);
        assert!(!err.is_permanent());
    }

    #[test]
    fn publish_failure_is_transient() {
        let err = AppError::Publish(br_util_nats_fabric::FabricError::Serialization(
            serde_json::from_str::<serde_json::Value>("{").unwrap_err(),
        ));
        assert!(!err.is_permanent());
    }

    #[test]
    fn conflict_retries_exhausted_is_transient() {
        let err = AppError::ConflictRetriesExhausted { attempts: 5 };
        assert!(!err.is_permanent());
    }

    #[test]
    fn missing_registry_head_is_transient() {
        let err = AppError::MissingRegistryHead;
        assert!(!err.is_permanent());
    }
}
