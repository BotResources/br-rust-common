use std::collections::HashSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::ScopeDeclarationError;
use crate::spec::{ScopeSpec, ServiceManifest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeDeclaration {
    manifest: ServiceManifest,
    scopes: Vec<ScopeSpec>,
}

impl<'de> Deserialize<'de> for ScopeDeclaration {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            manifest: ServiceManifest,
            scopes: Vec<ScopeSpec>,
        }
        let Wire { manifest, scopes } = Wire::deserialize(deserializer)?;
        Self::new(manifest, scopes).map_err(D::Error::custom)
    }
}

impl ScopeDeclaration {
    pub fn new(
        manifest: ServiceManifest,
        scopes: Vec<ScopeSpec>,
    ) -> Result<Self, ScopeDeclarationError> {
        let mut seen = HashSet::with_capacity(scopes.len());
        for spec in &scopes {
            if !spec.key.is_owned_by(&manifest.key) {
                return Err(ScopeDeclarationError::ScopePrefixMismatch {
                    scope_service: spec.key.service_segment().to_string(),
                    declaring_service: manifest.key.as_str().to_string(),
                });
            }
            if !seen.insert(spec.key.as_str()) {
                return Err(ScopeDeclarationError::DuplicateScopeInDeclaration {
                    key: spec.key.as_str().to_string(),
                });
            }
        }
        Ok(Self { manifest, scopes })
    }

    pub fn manifest(&self) -> &ServiceManifest {
        &self.manifest
    }

    pub fn scopes(&self) -> &[ScopeSpec] {
        &self.scopes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::key::ScopeKey;
    use crate::service::ServiceKey;

    fn manifest(service: &str) -> ServiceManifest {
        ServiceManifest::new(ServiceKey::new(service).unwrap(), "label", "desc")
    }

    fn spec(key: &str) -> ScopeSpec {
        ScopeSpec::new(ScopeKey::new(key).unwrap(), "l", "d", false)
    }

    #[test]
    fn accepts_owned_unique_scopes() {
        let decl = ScopeDeclaration::new(
            manifest("notifier"),
            vec![spec("notifier:read"), spec("notifier:write")],
        )
        .unwrap();
        assert_eq!(decl.scopes().len(), 2);
        assert_eq!(decl.manifest().key.as_str(), "notifier");
    }

    #[test]
    fn accepts_empty_scope_set() {
        let decl = ScopeDeclaration::new(manifest("notifier"), vec![]).unwrap();
        assert!(decl.scopes().is_empty());
    }

    #[test]
    fn rejects_scope_owned_by_another_service() {
        let err = ScopeDeclaration::new(
            manifest("notifier"),
            vec![spec("notifier:read"), spec("billing:read")],
        )
        .unwrap_err();
        assert_eq!(
            err,
            ScopeDeclarationError::ScopePrefixMismatch {
                scope_service: "billing".to_string(),
                declaring_service: "notifier".to_string(),
            }
        );
    }

    #[test]
    fn rejects_duplicate_scope() {
        let err = ScopeDeclaration::new(
            manifest("notifier"),
            vec![spec("notifier:read"), spec("notifier:read")],
        )
        .unwrap_err();
        assert_eq!(
            err,
            ScopeDeclarationError::DuplicateScopeInDeclaration {
                key: "notifier:read".to_string(),
            }
        );
    }

    #[test]
    fn rejects_atomically_on_first_failure() {
        let err = ScopeDeclaration::new(
            manifest("notifier"),
            vec![spec("billing:read"), spec("notifier:x"), spec("notifier:x")],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ScopeDeclarationError::ScopePrefixMismatch { .. }
        ));
    }

    #[test]
    fn serde_roundtrip() {
        let decl = ScopeDeclaration::new(
            manifest("notifier"),
            vec![spec("notifier:read"), spec("notifier:write")],
        )
        .unwrap();
        let json = serde_json::to_string(&decl).unwrap();
        let back: ScopeDeclaration = serde_json::from_str(&json).unwrap();
        assert_eq!(decl, back);
    }

    #[test]
    fn deserialize_rejects_prefix_mismatch() {
        let bad = r#"{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"billing:read","label_key":"l","description_key":"d","platform_only":false}]
        }"#;
        assert!(serde_json::from_str::<ScopeDeclaration>(bad).is_err());
    }

    #[test]
    fn deserialize_rejects_duplicate() {
        let bad = r#"{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[
                {"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false},
                {"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false}
            ]
        }"#;
        assert!(serde_json::from_str::<ScopeDeclaration>(bad).is_err());
    }
}
