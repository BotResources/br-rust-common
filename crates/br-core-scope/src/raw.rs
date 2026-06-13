use serde::{Deserialize, Serialize};

use crate::declaration::ScopeDeclaration;
use crate::error::ScopeDeclarationError;
use crate::key::ScopeKey;
use crate::service::ServiceKey;
use crate::spec::{ScopeSpec, ServiceManifest};

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

impl RawScopeDeclaration {
    pub fn validate(self) -> Result<ScopeDeclaration, ScopeDeclarationError> {
        let RawScopeDeclaration { manifest, scopes } = self;
        let service_key = ServiceKey::new(manifest.key.clone()).map_err(|validation| {
            ScopeDeclarationError::InvalidScopeKey {
                key: manifest.key,
                validation,
            }
        })?;
        let manifest =
            ServiceManifest::new(service_key, manifest.label_key, manifest.description_key);

        let mut specs = Vec::with_capacity(scopes.len());
        for raw in scopes {
            let key = ScopeKey::new(raw.key.clone()).map_err(|validation| {
                ScopeDeclarationError::InvalidScopeKey {
                    key: raw.key,
                    validation,
                }
            })?;
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
        let manifest = decl.manifest();
        Self {
            manifest: RawServiceManifest {
                key: manifest.key.as_str().to_string(),
                label_key: manifest.label_key.clone(),
                description_key: manifest.description_key.clone(),
            },
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
