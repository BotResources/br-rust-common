use std::collections::HashSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::ScopeDeclarationError;
use crate::implication::check_implications;
use crate::key::ScopeKey;
use crate::spec::{ScopeSpec, ScopeSpecV2, ServiceManifest};

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
        check_ownership_and_uniqueness(&manifest, scopes.iter().map(|spec| &spec.key))?;
        Ok(Self { manifest, scopes })
    }

    pub fn manifest(&self) -> &ServiceManifest {
        &self.manifest
    }

    pub fn scopes(&self) -> &[ScopeSpec] {
        &self.scopes
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeDeclarationV2 {
    manifest: ServiceManifest,
    scopes: Vec<ScopeSpecV2>,
}

impl<'de> Deserialize<'de> for ScopeDeclarationV2 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            manifest: ServiceManifest,
            scopes: Vec<ScopeSpecV2>,
        }
        let Wire { manifest, scopes } = Wire::deserialize(deserializer)?;
        Self::new(manifest, scopes).map_err(D::Error::custom)
    }
}

impl ScopeDeclarationV2 {
    pub fn new(
        manifest: ServiceManifest,
        scopes: Vec<ScopeSpecV2>,
    ) -> Result<Self, ScopeDeclarationError> {
        check_ownership_and_uniqueness(&manifest, scopes.iter().map(|spec| &spec.key))?;
        check_implications(&manifest, &scopes)?;
        Ok(Self { manifest, scopes })
    }

    pub fn manifest(&self) -> &ServiceManifest {
        &self.manifest
    }

    pub fn scopes(&self) -> &[ScopeSpecV2] {
        &self.scopes
    }
}

impl From<ScopeDeclaration> for ScopeDeclarationV2 {
    fn from(declaration: ScopeDeclaration) -> Self {
        let ScopeDeclaration { manifest, scopes } = declaration;
        Self {
            manifest,
            scopes: scopes.into_iter().map(ScopeSpecV2::from).collect(),
        }
    }
}

fn check_ownership_and_uniqueness<'a>(
    manifest: &ServiceManifest,
    keys: impl Iterator<Item = &'a ScopeKey>,
) -> Result<(), ScopeDeclarationError> {
    let mut seen = HashSet::new();
    for key in keys {
        if !key.is_owned_by(&manifest.key) {
            return Err(ScopeDeclarationError::ScopePrefixMismatch {
                scope_service: key.service_segment().to_string(),
                declaring_service: manifest.key.as_str().to_string(),
            });
        }
        if !seen.insert(key.as_str()) {
            return Err(ScopeDeclarationError::DuplicateScopeInDeclaration {
                key: key.as_str().to_string(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
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

    fn spec_v2(key: &str, implies: &[&str]) -> ScopeSpecV2 {
        ScopeSpecV2::new(ScopeKey::new(key).unwrap(), "l", "d", false)
            .with_implies(implies.iter().map(|k| ScopeKey::new(*k).unwrap()))
    }

    #[test]
    fn v2_accepts_owned_unique_scopes_with_implications() {
        let decl = ScopeDeclarationV2::new(
            manifest("notifier"),
            vec![
                spec_v2("notifier:admin", &["notifier:read"]).with_service_accounts_only(true),
                spec_v2("notifier:read", &[]),
            ],
        )
        .unwrap();
        assert_eq!(decl.scopes().len(), 2);
        assert!(decl.scopes()[0].service_accounts_only);
        assert_eq!(decl.manifest().key.as_str(), "notifier");
    }

    #[test]
    fn v2_keeps_the_v1_rules() {
        let mismatch =
            ScopeDeclarationV2::new(manifest("notifier"), vec![spec_v2("billing:read", &[])])
                .unwrap_err();
        assert!(matches!(
            mismatch,
            ScopeDeclarationError::ScopePrefixMismatch { .. }
        ));

        let duplicate = ScopeDeclarationV2::new(
            manifest("notifier"),
            vec![spec_v2("notifier:read", &[]), spec_v2("notifier:read", &[])],
        )
        .unwrap_err();
        assert!(matches!(
            duplicate,
            ScopeDeclarationError::DuplicateScopeInDeclaration { .. }
        ));
    }

    #[test]
    fn v2_checks_v1_rules_before_implications() {
        let err = ScopeDeclarationV2::new(
            manifest("notifier"),
            vec![
                spec_v2("notifier:a", &["notifier:a"]),
                spec_v2("billing:x", &[]),
            ],
        )
        .unwrap_err();
        assert!(matches!(
            err,
            ScopeDeclarationError::ScopePrefixMismatch { .. }
        ));
    }

    #[test]
    fn v2_rejects_bad_implications() {
        let unknown = ScopeDeclarationV2::new(
            manifest("notifier"),
            vec![spec_v2("notifier:admin", &["notifier:ghost"])],
        )
        .unwrap_err();
        assert!(matches!(
            unknown,
            ScopeDeclarationError::UnknownImpliedScope { .. }
        ));

        let foreign = ScopeDeclarationV2::new(
            manifest("notifier"),
            vec![spec_v2("notifier:admin", &["billing:read"])],
        )
        .unwrap_err();
        assert!(matches!(
            foreign,
            ScopeDeclarationError::ImpliedScopePrefixMismatch { .. }
        ));

        let cycle = ScopeDeclarationV2::new(
            manifest("notifier"),
            vec![
                spec_v2("notifier:a", &["notifier:b"]),
                spec_v2("notifier:b", &["notifier:a"]),
            ],
        )
        .unwrap_err();
        assert!(matches!(
            cycle,
            ScopeDeclarationError::ImplicationCycle { .. }
        ));
    }

    #[test]
    fn v2_serde_roundtrip() {
        let decl = ScopeDeclarationV2::new(
            manifest("notifier"),
            vec![
                spec_v2("notifier:admin", &["notifier:read"]).with_service_accounts_only(true),
                spec_v2("notifier:read", &[]),
            ],
        )
        .unwrap();
        let json = serde_json::to_string(&decl).unwrap();
        let back: ScopeDeclarationV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(decl, back);
    }

    #[test]
    fn v2_deserialize_rejects_a_cycle() {
        let bad = r#"{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"notifier:a","label_key":"l","description_key":"d","platform_only":false,"implies":["notifier:a"]}]
        }"#;
        assert!(serde_json::from_str::<ScopeDeclarationV2>(bad).is_err());
    }

    #[test]
    fn v1_declaration_upgrades_to_v2_with_defaults() {
        let v1 = ScopeDeclaration::new(
            manifest("notifier"),
            vec![spec("notifier:read"), spec("notifier:write")],
        )
        .unwrap();
        let v2 = ScopeDeclarationV2::from(v1.clone());
        assert_eq!(v2.manifest(), v1.manifest());
        assert_eq!(v2.scopes().len(), 2);
        for (upgraded, original) in v2.scopes().iter().zip(v1.scopes()) {
            assert_eq!(upgraded.key, original.key);
            assert!(upgraded.implies.is_empty());
            assert!(!upgraded.service_accounts_only);
        }
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
