use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct PublishedUser {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
}

impl PublishedUser {
    pub fn extension(&self, key: &str) -> Option<&Value> {
        self.extensions.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live_reference_wire() -> Value {
        serde_json::json!({
            "version": 1,
            "email": "ada@example.com",
            "first_name": "Ada",
            "last_name": "Lovelace",
            "avatar_object_key": "users/ada/avatar.png",
            "avatar_mime": "image/png",
            "locale": "en",
            "disabled_at": "2025-04-12T09:30:00Z",
            "x_custom": { "nested": "value" }
        })
    }

    #[test]
    fn deserializes_the_frozen_live_wire() {
        let user: PublishedUser = serde_json::from_value(live_reference_wire()).unwrap();
        assert_eq!(user.email, "ada@example.com");
        assert_eq!(user.first_name.as_deref(), Some("Ada"));
        assert_eq!(user.last_name.as_deref(), Some("Lovelace"));
    }

    #[test]
    fn round_trip_is_semantically_stable_against_live_wire() {
        let wire = live_reference_wire();
        let user: PublishedUser = serde_json::from_value(wire.clone()).unwrap();
        let back = serde_json::to_value(&user).unwrap();
        assert_eq!(wire, back);
    }

    #[test]
    fn project_fields_ride_as_extensions_not_core() {
        let user: PublishedUser = serde_json::from_value(live_reference_wire()).unwrap();
        assert_eq!(user.extension("locale"), Some(&Value::from("en")));
        assert_eq!(user.extension("version"), Some(&Value::from(1)));
        assert!(user.extension("avatar_object_key").is_some());
        assert!(user.extension("x_custom").is_some());
        assert!(user.extension("email").is_none());
        assert!(user.extension("first_name").is_none());
    }

    #[test]
    fn core_only_value_deserializes_with_empty_extensions() {
        let core = serde_json::json!({
            "email": "solo@example.com",
            "first_name": null,
            "last_name": null
        });
        let user: PublishedUser = serde_json::from_value(core).unwrap();
        assert_eq!(user.email, "solo@example.com");
        assert!(user.first_name.is_none());
        assert!(user.extensions.is_empty());
    }

    #[test]
    fn names_absent_in_wire_default_to_none() {
        let wire = serde_json::json!({ "email": "no-name@example.com" });
        let user: PublishedUser = serde_json::from_value(wire).unwrap();
        assert!(user.first_name.is_none());
        assert!(user.last_name.is_none());
    }
}
