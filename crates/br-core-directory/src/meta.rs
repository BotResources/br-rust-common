use serde::{Deserialize, Serialize};

pub const DIRECTORY_META_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishedEntity {
    Users,
    Groups,
    ServiceAccounts,
    Other(String),
}

impl PublishedEntity {
    pub fn as_wire(&self) -> &str {
        match self {
            PublishedEntity::Users => "users",
            PublishedEntity::Groups => "groups",
            PublishedEntity::ServiceAccounts => "service_accounts",
            PublishedEntity::Other(raw) => raw,
        }
    }

    pub fn from_wire(raw: &str) -> Self {
        match raw {
            "users" => PublishedEntity::Users,
            "groups" => PublishedEntity::Groups,
            "service_accounts" => PublishedEntity::ServiceAccounts,
            other => PublishedEntity::Other(other.to_string()),
        }
    }
}

impl Serialize for PublishedEntity {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_wire())
    }
}

impl<'de> Deserialize<'de> for PublishedEntity {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Ok(PublishedEntity::from_wire(&raw))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct DirectoryMeta {
    pub version: u8,
    pub entities: Vec<PublishedEntity>,
}

impl DirectoryMeta {
    pub fn publishes(&self, entity: &PublishedEntity) -> bool {
        self.entities.contains(entity)
    }

    pub fn publishes_users(&self) -> bool {
        self.publishes(&PublishedEntity::Users)
    }

    pub fn publishes_groups(&self) -> bool {
        self.publishes(&PublishedEntity::Groups)
    }

    pub fn publishes_service_accounts(&self) -> bool {
        self.publishes(&PublishedEntity::ServiceAccounts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn full_manifest_declares_users_and_groups() {
        let wire = json!({ "version": 1, "entities": ["users", "groups"] });
        let meta: DirectoryMeta = serde_json::from_value(wire).unwrap();
        assert_eq!(meta.version, DIRECTORY_META_VERSION);
        assert!(meta.publishes_users());
        assert!(meta.publishes_groups());
    }

    #[test]
    fn users_only_manifest_auto_degrades_groups() {
        let wire = json!({ "version": 1, "entities": ["users"] });
        let meta: DirectoryMeta = serde_json::from_value(wire).unwrap();
        assert!(meta.publishes_users());
        assert!(!meta.publishes_groups());
    }

    #[test]
    fn round_trip_is_stable() {
        let meta = DirectoryMeta {
            version: DIRECTORY_META_VERSION,
            entities: vec![PublishedEntity::Users, PublishedEntity::Groups],
        };
        let wire = serde_json::to_value(&meta).unwrap();
        assert_eq!(
            wire,
            json!({ "version": 1, "entities": ["users", "groups"] })
        );
        let back: DirectoryMeta = serde_json::from_value(wire).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn unknown_future_entity_is_captured_not_dropped() {
        let wire = json!({ "version": 2, "entities": ["users", "robots"] });
        let meta: DirectoryMeta = serde_json::from_value(wire).unwrap();
        assert!(meta.publishes_users());
        assert!(!meta.publishes_groups());
        assert!(meta.publishes(&PublishedEntity::Other("robots".to_string())));
        let back = serde_json::to_value(&meta).unwrap();
        assert_eq!(
            back,
            json!({ "version": 2, "entities": ["users", "robots"] })
        );
    }

    #[test]
    fn service_accounts_entity_round_trips_as_a_concrete_variant() {
        let wire = json!({ "version": 1, "entities": ["users", "service_accounts"] });
        let meta: DirectoryMeta = serde_json::from_value(wire.clone()).unwrap();
        assert!(meta.publishes_users());
        assert!(!meta.publishes_groups());
        assert!(meta.publishes_service_accounts());
        assert!(meta.publishes(&PublishedEntity::ServiceAccounts));
        let back = serde_json::to_value(&meta).unwrap();
        assert_eq!(wire, back);
    }
}
