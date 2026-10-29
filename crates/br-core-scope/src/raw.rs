use serde::{Deserialize, Serialize};

use crate::declaration::{ScopeDeclaration, ScopeDeclarationV2};
use crate::error::ScopeDeclarationError;
use crate::key::ScopeKey;
use crate::service::ServiceKey;
use crate::spec::{ScopeSpec, ScopeSpecV2, ServiceManifest};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScopeDeclaration {
    pub manifest: RawServiceManifest,
    pub scopes: Vec<RawScopeSpec>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawServiceManifest {
    pub key: String,
    pub label_key: String,
    pub description_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScopeSpec {
    pub key: String,
    pub label_key: String,
    pub description_key: String,
    pub platform_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScopeDeclarationV2 {
    pub manifest: RawServiceManifest,
    pub scopes: Vec<RawScopeSpecV2>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScopeSpecV2 {
    pub key: String,
    pub label_key: String,
    pub description_key: String,
    pub platform_only: bool,
    #[serde(default)]
    pub implies: Vec<String>,
    #[serde(default)]
    pub service_accounts_only: bool,
}

impl RawServiceManifest {
    fn validate(self) -> Result<ServiceManifest, ScopeDeclarationError> {
        let key = ServiceKey::new(self.key.clone()).map_err(|validation| {
            ScopeDeclarationError::InvalidScopeKey {
                key: self.key,
                validation,
            }
        })?;
        Ok(ServiceManifest::new(
            key,
            self.label_key,
            self.description_key,
        ))
    }

    fn from_manifest(manifest: &ServiceManifest) -> Self {
        Self {
            key: manifest.key.as_str().to_string(),
            label_key: manifest.label_key.clone(),
            description_key: manifest.description_key.clone(),
        }
    }
}

fn parse_scope_key(raw: String) -> Result<ScopeKey, ScopeDeclarationError> {
    ScopeKey::new(raw.clone()).map_err(|validation| ScopeDeclarationError::InvalidScopeKey {
        key: raw,
        validation,
    })
}

impl RawScopeDeclaration {
    pub fn validate(self) -> Result<ScopeDeclaration, ScopeDeclarationError> {
        let RawScopeDeclaration { manifest, scopes } = self;
        let manifest = manifest.validate()?;

        let mut specs = Vec::with_capacity(scopes.len());
        for raw in scopes {
            let key = parse_scope_key(raw.key)?;
            specs.push(ScopeSpec::new(
                key,
                raw.label_key,
                raw.description_key,
                raw.platform_only,
            ));
        }

        ScopeDeclaration::new(manifest, specs)
    }
}

impl From<&ScopeDeclaration> for RawScopeDeclaration {
    fn from(decl: &ScopeDeclaration) -> Self {
        Self {
            manifest: RawServiceManifest::from_manifest(decl.manifest()),
            scopes: decl
                .scopes()
                .iter()
                .map(|spec| RawScopeSpec {
                    key: spec.key.as_str().to_string(),
                    label_key: spec.label_key.clone(),
                    description_key: spec.description_key.clone(),
                    platform_only: spec.platform_only,
                })
                .collect(),
        }
    }
}

impl From<ScopeDeclaration> for RawScopeDeclaration {
    fn from(decl: ScopeDeclaration) -> Self {
        Self::from(&decl)
    }
}

impl RawScopeDeclarationV2 {
    pub fn validate(self) -> Result<ScopeDeclarationV2, ScopeDeclarationError> {
        let RawScopeDeclarationV2 { manifest, scopes } = self;
        let manifest = manifest.validate()?;

        let mut specs = Vec::with_capacity(scopes.len());
        for raw in scopes {
            let key = parse_scope_key(raw.key)?;
            let implies = raw
                .implies
                .into_iter()
                .map(parse_scope_key)
                .collect::<Result<Vec<_>, _>>()?;
            specs.push(
                ScopeSpecV2::new(key, raw.label_key, raw.description_key, raw.platform_only)
                    .with_implies(implies)
                    .with_service_accounts_only(raw.service_accounts_only),
            );
        }

        ScopeDeclarationV2::new(manifest, specs)
    }
}

impl From<&ScopeDeclarationV2> for RawScopeDeclarationV2 {
    fn from(decl: &ScopeDeclarationV2) -> Self {
        Self {
            manifest: RawServiceManifest::from_manifest(decl.manifest()),
            scopes: decl
                .scopes()
                .iter()
                .map(|spec| RawScopeSpecV2 {
                    key: spec.key.as_str().to_string(),
                    label_key: spec.label_key.clone(),
                    description_key: spec.description_key.clone(),
                    platform_only: spec.platform_only,
                    implies: spec
                        .implies
                        .iter()
                        .map(|implied| implied.as_str().to_string())
                        .collect(),
                    service_accounts_only: spec.service_accounts_only,
                })
                .collect(),
        }
    }
}

impl From<ScopeDeclarationV2> for RawScopeDeclarationV2 {
    fn from(decl: ScopeDeclarationV2) -> Self {
        Self::from(&decl)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KeyValidationError;

    fn valid_declaration() -> ScopeDeclaration {
        ScopeDeclaration::new(
            ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "svc.l", "svc.d"),
            vec![
                ScopeSpec::new(ScopeKey::new("notifier:read").unwrap(), "r.l", "r.d", false),
                ScopeSpec::new(ScopeKey::new("notifier:admin").unwrap(), "a.l", "a.d", true),
            ],
        )
        .unwrap()
    }

    fn raw_manifest(key: &str) -> RawServiceManifest {
        RawServiceManifest {
            key: key.to_string(),
            label_key: "l".to_string(),
            description_key: "d".to_string(),
        }
    }

    fn raw_spec(key: &str) -> RawScopeSpec {
        RawScopeSpec {
            key: key.to_string(),
            label_key: "l".to_string(),
            description_key: "d".to_string(),
            platform_only: false,
        }
    }

    #[test]
    fn validate_round_trips_a_valid_declaration() {
        let decl = valid_declaration();
        let raw = RawScopeDeclaration::from(&decl);
        let revalidated = raw.validate().unwrap();
        assert_eq!(decl, revalidated);
    }

    #[test]
    fn raw_json_is_byte_identical_to_validated() {
        let decl = valid_declaration();
        let from_validated = serde_json::to_string(&decl).unwrap();
        let from_raw = serde_json::to_string(&RawScopeDeclaration::from(&decl)).unwrap();
        assert_eq!(from_validated, from_raw);
    }

    #[test]
    fn validate_yields_structured_invalid_scope_key() {
        let json = r#"{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"notifier:BAD","label_key":"l","description_key":"d","platform_only":false}]
        }"#;
        let raw: RawScopeDeclaration = serde_json::from_str(json).unwrap();
        assert_eq!(
            raw.validate().unwrap_err(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "notifier:BAD".to_string(),
                validation: KeyValidationError::InvalidCharset,
            }
        );
        let raw = RawScopeDeclaration {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec("notifierread")],
        };
        assert_eq!(
            raw.validate().unwrap_err(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "notifierread".to_string(),
                validation: KeyValidationError::MalformedSegments,
            }
        );
    }

    #[test]
    fn validate_reports_malformed_manifest_key_as_invalid_scope_key() {
        let raw = RawScopeDeclaration {
            manifest: raw_manifest("NOPE"),
            scopes: vec![],
        };
        assert_eq!(
            raw.validate().unwrap_err(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "NOPE".to_string(),
                validation: KeyValidationError::InvalidCharset,
            }
        );
    }

    #[test]
    fn validate_delegates_cross_cutting_rules() {
        let mismatch = RawScopeDeclaration {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec("billing:read")],
        };
        assert_eq!(
            mismatch.validate().unwrap_err(),
            ScopeDeclarationError::ScopePrefixMismatch {
                scope_service: "billing".to_string(),
                declaring_service: "notifier".to_string(),
            }
        );

        let duplicate = RawScopeDeclaration {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec("notifier:read"), raw_spec("notifier:read")],
        };
        assert_eq!(
            duplicate.validate().unwrap_err(),
            ScopeDeclarationError::DuplicateScopeInDeclaration {
                key: "notifier:read".to_string(),
            }
        );
    }

    fn valid_declaration_v2() -> ScopeDeclarationV2 {
        ScopeDeclarationV2::new(
            ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "svc.l", "svc.d"),
            vec![
                ScopeSpecV2::new(ScopeKey::new("notifier:read").unwrap(), "r.l", "r.d", false),
                ScopeSpecV2::new(ScopeKey::new("notifier:admin").unwrap(), "a.l", "a.d", true)
                    .with_implies([ScopeKey::new("notifier:read").unwrap()])
                    .with_service_accounts_only(true),
            ],
        )
        .unwrap()
    }

    fn raw_spec_v2(key: &str, implies: &[&str]) -> RawScopeSpecV2 {
        RawScopeSpecV2 {
            key: key.to_string(),
            label_key: "l".to_string(),
            description_key: "d".to_string(),
            platform_only: false,
            implies: implies.iter().map(|k| k.to_string()).collect(),
            service_accounts_only: false,
        }
    }

    #[test]
    fn v2_validate_round_trips_a_valid_declaration() {
        let decl = valid_declaration_v2();
        let raw = RawScopeDeclarationV2::from(&decl);
        assert_eq!(raw.validate().unwrap(), decl);
    }

    #[test]
    fn v2_raw_json_is_byte_identical_to_validated() {
        let decl = valid_declaration_v2();
        let from_validated = serde_json::to_string(&decl).unwrap();
        let from_raw = serde_json::to_string(&RawScopeDeclarationV2::from(&decl)).unwrap();
        assert_eq!(from_validated, from_raw);
    }

    #[test]
    fn v2_minimal_payload_parses_with_defaults() {
        let json = r#"{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false}]
        }"#;
        let raw: RawScopeDeclarationV2 = serde_json::from_str(json).unwrap();
        assert!(raw.scopes[0].implies.is_empty());
        assert!(!raw.scopes[0].service_accounts_only);
        let decl = raw.validate().unwrap();
        assert!(decl.scopes()[0].implies.is_empty());
        assert!(!decl.scopes()[0].service_accounts_only);
    }

    #[test]
    fn v2_validate_yields_structured_invalid_implied_key() {
        let raw = RawScopeDeclarationV2 {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec_v2("notifier:admin", &["notifier:BAD"])],
        };
        assert_eq!(
            raw.validate().unwrap_err(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "notifier:BAD".to_string(),
                validation: KeyValidationError::InvalidCharset,
            }
        );
    }

    #[test]
    fn v2_validate_yields_structured_implication_reasons() {
        let unknown = RawScopeDeclarationV2 {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec_v2("notifier:admin", &["notifier:ghost"])],
        };
        assert_eq!(
            unknown.validate().unwrap_err(),
            ScopeDeclarationError::UnknownImpliedScope {
                key: "notifier:admin".to_string(),
                implied: "notifier:ghost".to_string(),
            }
        );

        let foreign = RawScopeDeclarationV2 {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec_v2("notifier:admin", &["billing:read"])],
        };
        assert_eq!(
            foreign.validate().unwrap_err(),
            ScopeDeclarationError::ImpliedScopePrefixMismatch {
                key: "notifier:admin".to_string(),
                implied: "billing:read".to_string(),
                declaring_service: "notifier".to_string(),
            }
        );

        let cycle = RawScopeDeclarationV2 {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec_v2("notifier:admin", &["notifier:admin"])],
        };
        assert_eq!(
            cycle.validate().unwrap_err(),
            ScopeDeclarationError::ImplicationCycle {
                cycle: vec!["notifier:admin".to_string(), "notifier:admin".to_string()],
            }
        );
    }

    #[test]
    fn validate_reports_invalid_key_before_prefix_mismatch() {
        let raw = RawScopeDeclaration {
            manifest: raw_manifest("notifier"),
            scopes: vec![raw_spec("BAD KEY")],
        };
        assert!(matches!(
            raw.validate().unwrap_err(),
            ScopeDeclarationError::InvalidScopeKey { .. }
        ));
    }
}
