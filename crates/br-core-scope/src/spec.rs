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
#[non_exhaustive]
pub struct ScopeSpecV2 {
    pub key: ScopeKey,
    pub label_key: String,
    pub description_key: String,
    pub platform_only: bool,
    #[serde(default)]
    pub implies: Vec<ScopeKey>,
    #[serde(default)]
    pub service_accounts_only: bool,
}

impl ScopeSpecV2 {
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
            implies: Vec::new(),
            service_accounts_only: false,
        }
    }

    pub fn with_implies(mut self, implies: impl IntoIterator<Item = ScopeKey>) -> Self {
        self.implies = implies.into_iter().collect();
        self
    }

    pub fn with_service_accounts_only(mut self, service_accounts_only: bool) -> Self {
        self.service_accounts_only = service_accounts_only;
        self
    }
}

impl From<ScopeSpec> for ScopeSpecV2 {
    fn from(spec: ScopeSpec) -> Self {
        Self::new(
            spec.key,
            spec.label_key,
            spec.description_key,
            spec.platform_only,
        )
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
    fn scope_spec_v2_defaults_to_no_implication_and_any_principal() {
        let spec = ScopeSpecV2::new(ScopeKey::new("notifier:read").unwrap(), "l", "d", false);
        assert!(spec.implies.is_empty());
        assert!(!spec.service_accounts_only);
    }

    #[test]
    fn scope_spec_v2_builders_set_the_new_fields() {
        let spec = ScopeSpecV2::new(ScopeKey::new("notifier:admin").unwrap(), "l", "d", true)
            .with_implies([ScopeKey::new("notifier:read").unwrap()])
            .with_service_accounts_only(true);
        assert_eq!(spec.implies, vec![ScopeKey::new("notifier:read").unwrap()]);
        assert!(spec.service_accounts_only);
        assert!(spec.platform_only);
    }

    #[test]
    fn scope_spec_v2_wire_shape_carries_the_new_fields() {
        let spec = ScopeSpecV2::new(ScopeKey::new("notifier:admin").unwrap(), "l", "d", false)
            .with_implies([ScopeKey::new("notifier:read").unwrap()])
            .with_service_accounts_only(true);
        let json = serde_json::to_value(&spec).unwrap();
        assert_eq!(json["key"], "notifier:admin");
        assert_eq!(json["implies"], serde_json::json!(["notifier:read"]));
        assert_eq!(json["service_accounts_only"], true);
        let back: ScopeSpecV2 = serde_json::from_value(json).unwrap();
        assert_eq!(spec, back);
    }

    #[test]
    fn scope_spec_v2_minimal_payload_takes_the_defaults() {
        let minimal = r#"{"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false}"#;
        let spec: ScopeSpecV2 = serde_json::from_str(minimal).unwrap();
        assert!(spec.implies.is_empty());
        assert!(!spec.service_accounts_only);
    }

    #[test]
    fn scope_spec_v2_deserialize_rejects_malformed_implied_key() {
        let bad = r#"{"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false,"implies":["BAD"]}"#;
        assert!(serde_json::from_str::<ScopeSpecV2>(bad).is_err());
    }

    #[test]
    fn scope_spec_v2_from_v1_keeps_the_v1_fields() {
        let v1 = ScopeSpec::new(ScopeKey::new("notifier:admin").unwrap(), "l", "d", true);
        let v2 = ScopeSpecV2::from(v1.clone());
        assert_eq!(v2.key, v1.key);
        assert_eq!(v2.label_key, v1.label_key);
        assert_eq!(v2.description_key, v1.description_key);
        assert!(v2.platform_only);
        assert!(v2.implies.is_empty());
        assert!(!v2.service_accounts_only);
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
