use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{DirectoryError, reject_reserved_keys};

pub const PUBLISHED_USER_RESERVED_KEYS: [&str; 3] = ["email", "first_name", "last_name"];

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PublishedUser {
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl PublishedUser {
    pub fn new(
        email: String,
        first_name: Option<String>,
        last_name: Option<String>,
        extensions: BTreeMap<String, Value>,
    ) -> Result<Self, DirectoryError> {
        reject_reserved_keys("PublishedUser", PUBLISHED_USER_RESERVED_KEYS, &extensions)?;
        Ok(Self {
            email,
            first_name,
            last_name,
            extensions,
        })
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    pub fn extension(&self, key: &str) -> Option<&Value> {
        self.extensions.get(key)
    }
}

impl<'de> Deserialize<'de> for PublishedUser {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut bag = BTreeMap::<String, Value>::deserialize(deserializer)?;
        let email = crate::flatten::take_required_string(&mut bag, "email")
            .map_err(serde::de::Error::custom)?;
        let first_name = crate::flatten::take_optional_string(&mut bag, "first_name")
            .map_err(serde::de::Error::custom)?;
        let last_name = crate::flatten::take_optional_string(&mut bag, "last_name")
            .map_err(serde::de::Error::custom)?;
        PublishedUser::new(email, first_name, last_name, bag).map_err(serde::de::Error::custom)
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
        assert!(user.extensions().is_empty());
    }

    #[test]
    fn names_absent_in_wire_default_to_none() {
        let wire = serde_json::json!({ "email": "no-name@example.com" });
        let user: PublishedUser = serde_json::from_value(wire).unwrap();
        assert!(user.first_name.is_none());
        assert!(user.last_name.is_none());
    }

    #[test]
    fn new_rejects_extension_shadowing_a_reserved_key() {
        let mut extensions = BTreeMap::new();
        extensions.insert("last_name".to_string(), Value::from("Shadow"));
        let err =
            PublishedUser::new("a@example.com".to_string(), None, None, extensions).unwrap_err();
        assert_eq!(
            err,
            DirectoryError::ReservedExtensionKey {
                entity: "PublishedUser",
                key: "last_name".to_string(),
            }
        );
    }

    #[test]
    fn new_accepts_a_non_reserved_extension() {
        let mut extensions = BTreeMap::new();
        extensions.insert("locale".to_string(), Value::from("fr"));
        let user = PublishedUser::new("a@example.com".to_string(), None, None, extensions).unwrap();
        assert_eq!(user.extension("locale"), Some(&Value::from("fr")));
    }
}
