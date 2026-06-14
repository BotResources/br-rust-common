use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PublishedGroup {
    pub name: String,
    pub member_ids: Vec<Uuid>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl PublishedGroup {
    pub fn extension(&self, key: &str) -> Option<&Value> {
        self.extensions.get(key)
    }

    pub fn has_member(&self, user_id: Uuid) -> bool {
        self.member_ids.contains(&user_id)
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
        assert!(group.extensions.is_empty());
    }
}
