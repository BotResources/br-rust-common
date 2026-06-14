use uuid::Uuid;

pub const USERS_KEY_PREFIX: &str = "identity/users/";
pub const GROUPS_KEY_PREFIX: &str = "identity/groups/";
pub const META_KEY: &str = "identity/_meta";

pub fn user_kv_key(user_id: Uuid) -> String {
    format!("{USERS_KEY_PREFIX}{user_id}")
}

pub fn group_kv_key(group_id: Uuid) -> String {
    format!("{GROUPS_KEY_PREFIX}{group_id}")
}

pub fn user_id_from_kv_key(key: &str) -> Option<Uuid> {
    key.strip_prefix(USERS_KEY_PREFIX)
        .and_then(|id| Uuid::parse_str(id).ok())
}

pub fn group_id_from_kv_key(key: &str) -> Option<Uuid> {
    key.strip_prefix(GROUPS_KEY_PREFIX)
        .and_then(|id| Uuid::parse_str(id).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_key_uses_frozen_prefix() {
        let id = Uuid::nil();
        assert_eq!(user_kv_key(id), format!("identity/users/{id}"));
    }

    #[test]
    fn group_key_uses_frozen_prefix() {
        let id = Uuid::nil();
        assert_eq!(group_kv_key(id), format!("identity/groups/{id}"));
    }

    #[test]
    fn user_id_round_trips_through_its_key() {
        let id = Uuid::from_u128(0x0193_8c1f_0000_7000_8000_0000_0000_0001);
        assert_eq!(user_id_from_kv_key(&user_kv_key(id)), Some(id));
    }

    #[test]
    fn group_id_round_trips_through_its_key() {
        let id = Uuid::from_u128(0x0193_8c1f_0000_7000_8000_0000_0000_0002);
        assert_eq!(group_id_from_kv_key(&group_kv_key(id)), Some(id));
    }

    #[test]
    fn id_parse_rejects_wrong_prefix() {
        assert_eq!(user_id_from_kv_key("identity/groups/x"), None);
        assert_eq!(group_id_from_kv_key("identity/users/x"), None);
        assert_eq!(user_id_from_kv_key(META_KEY), None);
    }

    #[test]
    fn id_parse_rejects_non_uuid_suffix() {
        assert_eq!(user_id_from_kv_key("identity/users/not-a-uuid"), None);
        assert_eq!(group_id_from_kv_key("identity/groups/"), None);
    }
}
