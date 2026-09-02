use uuid::Uuid;

use crate::error::DirectoryError;

pub const USER_NAMESPACE: &str = "identity.user";
pub const GROUP_NAMESPACE: &str = "identity.group";
pub const SERVICE_ACCOUNT_NAMESPACE: &str = "identity.service_account";

const MAX_NAMESPACE_LEN: usize = 64;
const MAX_KEY_LEN: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ForeignRef {
    namespace: String,
    key: String,
}

impl ForeignRef {
    pub fn new(namespace: &str, key: &str) -> Result<Self, DirectoryError> {
        if !is_valid_namespace(namespace) {
            return Err(DirectoryError::InvalidForeignNamespace);
        }
        if !is_valid_key(key) {
            return Err(DirectoryError::InvalidForeignKey);
        }
        Ok(Self {
            namespace: namespace.to_string(),
            key: key.to_string(),
        })
    }

    pub(crate) fn user(user_id: Uuid) -> Self {
        Self::entity(USER_NAMESPACE, user_id)
    }

    pub(crate) fn group(group_id: Uuid) -> Self {
        Self::entity(GROUP_NAMESPACE, group_id)
    }

    pub(crate) fn service_account(service_account_id: Uuid) -> Self {
        Self::entity(SERVICE_ACCOUNT_NAMESPACE, service_account_id)
    }

    fn entity(namespace: &'static str, id: Uuid) -> Self {
        Self {
            namespace: namespace.to_string(),
            key: id.to_string(),
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn key(&self) -> &str {
        &self.key
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Impact {
    ForeignChanged { foreign: ForeignRef },
}

#[async_trait::async_trait]
pub trait ImpactStager: Send + Sync {
    async fn stage_in(
        &self,
        conn: &mut sqlx::PgConnection,
        impacts: &[Impact],
    ) -> Result<(), DirectoryError>;
}

fn is_valid_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace.len() <= MAX_NAMESPACE_LEN
        && !namespace.starts_with('.')
        && !namespace.ends_with('.')
        && !namespace.contains("..")
        && namespace
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_')
}

fn is_valid_key(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= MAX_KEY_LEN
        && !key.chars().any(|c| c.is_control() || c.is_whitespace())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frozen_namespace_and_a_uuid_key_build_a_foreign_ref() {
        let foreign = ForeignRef::new(USER_NAMESPACE, "018f0000-0000-7000-8000-000000000000")
            .expect("valid foreign ref");
        assert_eq!(foreign.namespace(), "identity.user");
        assert_eq!(foreign.key(), "018f0000-0000-7000-8000-000000000000");
    }

    #[test]
    fn the_three_directory_namespaces_are_valid() {
        for namespace in [USER_NAMESPACE, GROUP_NAMESPACE, SERVICE_ACCOUNT_NAMESPACE] {
            assert!(ForeignRef::new(namespace, "k").is_ok(), "{namespace}");
        }
    }

    #[test]
    fn the_entity_constructors_agree_with_the_validating_constructor() {
        let id = Uuid::from_u128(42);
        assert_eq!(
            ForeignRef::user(id),
            ForeignRef::new(USER_NAMESPACE, &id.to_string()).expect("valid")
        );
        assert_eq!(
            ForeignRef::group(id),
            ForeignRef::new(GROUP_NAMESPACE, &id.to_string()).expect("valid")
        );
        assert_eq!(
            ForeignRef::service_account(id),
            ForeignRef::new(SERVICE_ACCOUNT_NAMESPACE, &id.to_string()).expect("valid")
        );
    }

    #[test]
    fn a_malformed_namespace_is_refused_at_construction() {
        for namespace in [
            "",
            ".identity",
            "identity.",
            "identity..user",
            "Identity.User",
            "identity user",
            "identity/user",
            &"n".repeat(MAX_NAMESPACE_LEN + 1),
        ] {
            assert!(
                matches!(
                    ForeignRef::new(namespace, "k"),
                    Err(DirectoryError::InvalidForeignNamespace)
                ),
                "{namespace:?} should be refused"
            );
        }
    }

    #[test]
    fn a_malformed_key_is_refused_at_construction() {
        for key in ["", " ", "a b", "a\nb", "a\0b", &"k".repeat(MAX_KEY_LEN + 1)] {
            assert!(
                matches!(
                    ForeignRef::new(USER_NAMESPACE, key),
                    Err(DirectoryError::InvalidForeignKey)
                ),
                "{key:?} should be refused"
            );
        }
    }
}
