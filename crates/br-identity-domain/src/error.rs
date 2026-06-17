use br_core_scope::{ScopeKey, ServiceKey};

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RegistryHydrationError {
    #[error("duplicate_scope_in_registry")]
    DuplicateScope { key: ScopeKey },
    #[error("duplicate_service_in_registry")]
    DuplicateService { key: ServiceKey },
    #[error("scope_owner_mismatch")]
    ScopeOwnerMismatch {
        key: ScopeKey,
        owning_service: ServiceKey,
    },
}
