use br_core_directory::{GROUPS_KEY_PREFIX, SERVICE_ACCOUNTS_KEY_PREFIX, USERS_KEY_PREFIX};
use br_util_nats_fabric::KvPrefix;

#[cfg(feature = "publisher")]
use br_core_directory::{META_KEY, group_kv_key, service_account_kv_key, user_kv_key};
#[cfg(feature = "publisher")]
use br_util_nats_fabric::{KvKey, KvKeyError};
#[cfg(feature = "publisher")]
use uuid::Uuid;

pub(crate) fn users_prefix() -> KvPrefix {
    KvPrefix::new(USERS_KEY_PREFIX).expect("frozen users prefix is a valid kv prefix")
}

pub(crate) fn groups_prefix() -> KvPrefix {
    KvPrefix::new(GROUPS_KEY_PREFIX).expect("frozen groups prefix is a valid kv prefix")
}

pub(crate) fn service_accounts_prefix() -> KvPrefix {
    KvPrefix::new(SERVICE_ACCOUNTS_KEY_PREFIX)
        .expect("frozen service accounts prefix is a valid kv prefix")
}

#[cfg(feature = "publisher")]
pub(crate) fn meta_key() -> KvKey {
    KvKey::new(META_KEY).expect("frozen meta key is a valid kv key")
}

#[cfg(feature = "publisher")]
pub(crate) fn user_key(user_id: Uuid) -> Result<KvKey, KvKeyError> {
    KvKey::new(user_kv_key(user_id))
}

#[cfg(feature = "publisher")]
pub(crate) fn group_key(group_id: Uuid) -> Result<KvKey, KvKeyError> {
    KvKey::new(group_kv_key(group_id))
}

#[cfg(feature = "publisher")]
pub(crate) fn service_account_key(service_account_id: Uuid) -> Result<KvKey, KvKeyError> {
    KvKey::new(service_account_kv_key(service_account_id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_prefixes_are_the_frozen_identity_prefixes() {
        assert_eq!(users_prefix().as_str(), "identity/users/");
        assert_eq!(groups_prefix().as_str(), "identity/groups/");
        assert_eq!(
            service_accounts_prefix().as_str(),
            "identity/service_accounts/"
        );
    }

    #[cfg(feature = "publisher")]
    #[test]
    fn entity_keys_render_under_their_prefix() {
        let id = uuid::Uuid::from_u128(0x0193_8c1f_0000_7000_8000_0000_0000_0001);
        assert!(users_prefix().matches(user_key(id).unwrap().as_str()));
        assert!(groups_prefix().matches(group_key(id).unwrap().as_str()));
        assert!(service_accounts_prefix().matches(service_account_key(id).unwrap().as_str()));
    }

    #[cfg(feature = "publisher")]
    #[test]
    fn meta_key_is_the_frozen_manifest_key() {
        assert_eq!(meta_key().as_str(), "identity/_meta");
    }
}
