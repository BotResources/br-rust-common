use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{DirectoryError, reject_reserved_keys};

pub const PUBLISHED_SERVICE_ACCOUNT_RESERVED_KEYS: [&str; 1] = ["name"];

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PublishedServiceAccount {
    pub name: String,
    #[serde(flatten)]
    extensions: BTreeMap<String, Value>,
}

impl PublishedServiceAccount {
    pub fn new(name: String, extensions: BTreeMap<String, Value>) -> Result<Self, DirectoryError> {
        reject_reserved_keys(
            "PublishedServiceAccount",
            PUBLISHED_SERVICE_ACCOUNT_RESERVED_KEYS,
            &extensions,
        )?;
        Ok(Self { name, extensions })
    }

    pub fn extensions(&self) -> &BTreeMap<String, Value> {
        &self.extensions
    }

    pub fn extension(&self, key: &str) -> Option<&Value> {
        self.extensions.get(key)
    }
}

impl<'de> Deserialize<'de> for PublishedServiceAccount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let mut bag = BTreeMap::<String, Value>::deserialize(deserializer)?;
        let name = crate::flatten::take_required_string(&mut bag, "name")
            .map_err(serde::de::Error::custom)?;
        PublishedServiceAccount::new(name, bag).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_with_extensions() {
        let wire = serde_json::json!({
            "version": 1,
            "name": "ci-runner",
            "created_at": "2025-04-12T09:30:00Z"
        });
        let account: PublishedServiceAccount = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(account.name, "ci-runner");
        assert_eq!(account.extension("version"), Some(&Value::from(1)));
        assert!(account.extension("name").is_none());
        let back = serde_json::to_value(&account).unwrap();
        assert_eq!(wire, back);
    }

    #[test]
    fn round_trips_without_extensions() {
        let wire = serde_json::json!({ "name": "minimal" });
        let account: PublishedServiceAccount = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(account.name, "minimal");
        assert!(account.extensions().is_empty());
        let back = serde_json::to_value(&account).unwrap();
        assert_eq!(wire, back);
    }

    #[test]
    fn new_rejects_extension_shadowing_a_reserved_key() {
        let mut extensions = BTreeMap::new();
        extensions.insert("name".to_string(), Value::from("Shadow"));
        let err = PublishedServiceAccount::new("real".to_string(), extensions).unwrap_err();
        assert_eq!(
            err,
            DirectoryError::ReservedExtensionKey {
                entity: "PublishedServiceAccount",
                key: "name".to_string(),
            }
        );
    }

    #[test]
    fn deser_fails_closed_on_name_typed_as_non_string() {
        let wire = serde_json::json!({ "name": { "nested": true } });
        let result: Result<PublishedServiceAccount, _> = serde_json::from_value(wire);
        assert!(result.is_err());
    }

    #[test]
    fn deser_fails_closed_on_missing_name() {
        let wire = serde_json::json!({ "version": 1 });
        let result: Result<PublishedServiceAccount, _> = serde_json::from_value(wire);
        assert!(result.is_err());
    }
}
