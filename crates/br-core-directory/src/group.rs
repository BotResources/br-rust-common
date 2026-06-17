use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

use crate::error::{DirectoryError, reject_reserved_keys};

pub const PUBLISHED_GROUP_RESERVED_KEYS: [&str; 2] = ["name", "member_ids"];

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PublishedGroup {
    pub name: String,
    pub member_ids: Vec<Uuid>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl PublishedGroup {
    pub fn new(
        name: String,
        member_ids: Vec<Uuid>,
        extensions: BTreeMap<String, Value>,
    ) -> Result<Self, DirectoryError> {
        reject_reserved_keys("PublishedGroup", PUBLISHED_GROUP_RESERVED_KEYS, &extensions)?;
        Ok(Self {
            name,
            member_ids,
            extensions,
        })
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    pub fn extension(&self, key: &str) -> Option<&Value> {
        self.extensions.get(key)
    }

    pub fn has_member(&self, user_id: Uuid) -> bool {
        self.member_ids.contains(&user_id)
    }
}

impl<'de> Deserialize<'de> for PublishedGroup {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut bag = BTreeMap::<String, Value>::deserialize(deserializer)?;
        let name = crate::flatten::take_required_string(&mut bag, "name")
            .map_err(serde::de::Error::custom)?;
        let member_ids = crate::flatten::take_required_uuid_vec(&mut bag, "member_ids")
            .map_err(serde::de::Error::custom)?;
        PublishedGroup::new(name, member_ids, bag).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_reference_wire() -> Value {
        serde_json::json!({
            "version": 1,
            "name": "engineering",
            "x_custom": "00000000-0000-0000-0000-000000000000",
            "is_system": false,
            "member_ids": [
                "01938c1f-0000-7000-8000-000000000001",
                "01938c1f-0000-7000-8000-000000000002"
            ]
        })
    }

    #[test]
    fn deserializes_the_frozen_live_wire() {
        let group: PublishedGroup = serde_json::from_value(live_reference_wire()).unwrap();
        assert_eq!(group.name, "engineering");
        assert_eq!(group.member_ids.len(), 2);
    }

    #[test]
    fn round_trip_is_semantically_stable_against_live_wire() {
        let wire = live_reference_wire();
        let group: PublishedGroup = serde_json::from_value(wire.clone()).unwrap();
        let back = serde_json::to_value(&group).unwrap();
        assert_eq!(wire, back);
    }

    #[test]
    fn project_fields_ride_as_extensions_not_core() {
        let group: PublishedGroup = serde_json::from_value(live_reference_wire()).unwrap();
        assert_eq!(
            group.extension("x_custom"),
            Some(&Value::from("00000000-0000-0000-0000-000000000000"))
        );
        assert_eq!(group.extension("is_system"), Some(&Value::from(false)));
        assert_eq!(group.extension("version"), Some(&Value::from(1)));
        assert!(group.extension("name").is_none());
        assert!(group.extension("member_ids").is_none());
    }

    #[test]
    fn has_member_is_derivable_from_member_ids() {
        let group: PublishedGroup = serde_json::from_value(live_reference_wire()).unwrap();
        let member: Uuid = "01938c1f-0000-7000-8000-000000000001".parse().unwrap();
        let stranger: Uuid = "01938c1f-0000-7000-8000-0000000000ff".parse().unwrap();
        assert!(group.has_member(member));
        assert!(!group.has_member(stranger));
    }

    #[test]
    fn core_only_value_deserializes_with_empty_extensions() {
        let core = serde_json::json!({ "name": "core-only", "member_ids": [] });
        let group: PublishedGroup = serde_json::from_value(core).unwrap();
        assert_eq!(group.name, "core-only");
        assert!(group.member_ids.is_empty());
        assert!(group.extensions().is_empty());
    }

    #[test]
    fn new_rejects_extension_shadowing_a_reserved_key() {
        let mut extensions = BTreeMap::new();
        extensions.insert("name".to_string(), Value::from("Shadow"));
        let err = PublishedGroup::new("real".to_string(), vec![], extensions).unwrap_err();
        assert_eq!(
            err,
            DirectoryError::ReservedExtensionKey {
                entity: "PublishedGroup",
                key: "name".to_string(),
            }
        );
    }

    #[test]
    fn deser_fails_closed_on_member_ids_typed_as_non_uuid() {
        let wire = serde_json::json!({ "name": "bad", "member_ids": ["not-a-uuid"] });
        let result: Result<PublishedGroup, _> = serde_json::from_value(wire);
        assert!(result.is_err());
    }
}
