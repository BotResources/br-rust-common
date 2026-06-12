use serde::{Deserialize, Serialize};

use crate::key::ScopeKey;
use crate::service::ServiceKey;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeSpec {
    pub key: ScopeKey,
    pub label_key: String,
    pub description_key: String,
    pub platform_only: bool,
}

impl ScopeSpec {
    pub fn new(
        key: ScopeKey,
        label_key: impl Into<String>,
        description_key: impl Into<String>,
        platform_only: bool,
    ) -> Self {
        Self {
            key,
            label_key: label_key.into(),
            description_key: description_key.into(),
            platform_only,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceManifest {
    pub key: ServiceKey,
    pub label_key: String,
    pub description_key: String,
}

impl ServiceManifest {
    pub fn new(
        key: ServiceKey,
        label_key: impl Into<String>,
        description_key: impl Into<String>,
    ) -> Self {
        Self {
            key,
            label_key: label_key.into(),
            description_key: description_key.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope(key: &str) -> ScopeSpec {
        ScopeSpec::new(
            ScopeKey::new(key).unwrap(),
            "label.read",
            "desc.read",
            false,
        )
    }

    #[test]
    fn scope_spec_carries_its_parts() {
        let spec = scope("notifier:read");
        assert_eq!(spec.key.as_str(), "notifier:read");
        assert_eq!(spec.label_key, "label.read");
        assert_eq!(spec.description_key, "desc.read");
        assert!(!spec.platform_only);
    }

    #[test]
    fn scope_spec_serde_roundtrip() {
        let spec = ScopeSpec::new(
            ScopeKey::new("notifier:admin").unwrap(),
            "label.admin",
            "desc.admin",
            true,
        );
        let json = serde_json::to_string(&spec).unwrap();
        let back: ScopeSpec = serde_json::from_str(&json).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn scope_spec_wire_shape_embeds_key_as_string() {
        let spec = scope("notifier:read");
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["key"], "notifier:read");
        assert_eq!(json["platform_only"], false);
    }

    #[test]
    fn scope_spec_deserialize_rejects_malformed_key() {
        let bad = r#"{"key":"BAD","label_key":"l","description_key":"d","platform_only":false}"#;
        assert!(serde_json::from_str::<ScopeSpec>(bad).is_err());
    }

    #[test]
    fn service_manifest_serde_roundtrip() {
        let manifest = ServiceManifest::new(
            ServiceKey::new("notifier").unwrap(),
            "label.svc",
            "desc.svc",
        );
        let json = serde_json::to_string(&manifest).unwrap();
        let back: ServiceManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, back);
    }

    #[test]
    fn service_manifest_wire_shape_embeds_key_as_string() {
        let manifest = ServiceManifest::new(
            ServiceKey::new("notifier").unwrap(),
            "label.svc",
            "desc.svc",
        );
        let json = serde_json::to_value(&manifest).unwrap();
        assert_eq!(json["key"], "notifier");
    }
}
