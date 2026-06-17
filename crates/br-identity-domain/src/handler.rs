use br_core_scope::{DeclareServiceScopes, ScopeDeclarationError, ServiceKey};

use crate::event::CommandResult;
use crate::identity::RejectedIdentity;
use crate::registry::ScopeRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclarationOutcome {
    Accepted {
        service: ServiceKey,
        result: CommandResult,
    },
    Rejected {
        identity: RejectedIdentity,
        reason: ScopeDeclarationError,
    },
}

pub fn judge_declaration(
    registry: &mut ScopeRegistry,
    command: DeclareServiceScopes,
) -> DeclarationOutcome {
    let raw_key = command.raw().manifest.key.clone();
    let declaration = match command.validate() {
        Ok(declaration) => declaration,
        Err(reason) => {
            return DeclarationOutcome::Rejected {
                identity: RejectedIdentity::from_raw_key(&raw_key),
                reason,
            };
        }
    };
    let service = declaration.manifest().key.clone();
    match registry.register_declaration(&declaration) {
        Ok(result) => DeclarationOutcome::Accepted { service, result },
        Err(reason) => DeclarationOutcome::Rejected {
            identity: RejectedIdentity::Service(service),
            reason,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_scope::{
        KeyValidationError, ScopeDeclaration, ScopeKey, ScopeSpec, ServiceManifest,
    };

    use crate::event::RegistryEvent;

    fn declare(service: &str, scope_keys: &[&str]) -> DeclareServiceScopes {
        let scopes = scope_keys
            .iter()
            .map(|k| ScopeSpec::new(ScopeKey::new(*k).unwrap(), "l", "d", false))
            .collect();
        let manifest = ServiceManifest::new(ServiceKey::new(service).unwrap(), "l", "d");
        DeclareServiceScopes::new(ScopeDeclaration::new(manifest, scopes).unwrap())
    }

    fn raw_declare(json: &str) -> DeclareServiceScopes {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn valid_declaration_is_accepted_and_mutates_the_registry() {
        let mut registry = ScopeRegistry::new();
        let outcome = judge_declaration(&mut registry, declare("notifier", &["notifier:read"]));
        match outcome {
            DeclarationOutcome::Accepted { service, result } => {
                assert_eq!(service, ServiceKey::new("notifier").unwrap());
                assert_eq!(result.events.len(), 2);
                assert!(matches!(
                    result.events[1],
                    RegistryEvent::ScopeRegistered { .. }
                ));
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
        assert_eq!(registry.version(), 1);
    }

    #[test]
    fn idempotent_redeclaration_is_accepted_as_a_noop() {
        let mut registry = ScopeRegistry::new();
        judge_declaration(&mut registry, declare("notifier", &["notifier:read"]));
        let outcome = judge_declaration(&mut registry, declare("notifier", &["notifier:read"]));
        match outcome {
            DeclarationOutcome::Accepted { result, .. } => assert!(result.is_noop()),
            other => panic!("expected Accepted no-op, got {other:?}"),
        }
        assert_eq!(registry.version(), 1, "a no-op must not bump the version");
    }

    #[test]
    fn malformed_scope_key_is_rejected_with_invalid_scope_key() {
        let mut registry = ScopeRegistry::new();
        let before = registry.clone();
        let payload = raw_declare(
            r#"{"declaration":{
                "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
                "scopes":[{"key":"notifier:BAD","label_key":"l","description_key":"d","platform_only":false}]
            }}"#,
        );
        let outcome = judge_declaration(&mut registry, payload);
        assert_eq!(
            outcome,
            DeclarationOutcome::Rejected {
                identity: RejectedIdentity::Service(ServiceKey::new("notifier").unwrap()),
                reason: ScopeDeclarationError::InvalidScopeKey {
                    key: "notifier:BAD".to_string(),
                    validation: KeyValidationError::InvalidCharset,
                }
            }
        );
        assert_eq!(registry, before, "a rejection must not touch the registry");
    }

    #[test]
    fn prefix_mismatch_is_rejected() {
        let mut registry = ScopeRegistry::new();
        let payload = raw_declare(
            r#"{"declaration":{
                "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
                "scopes":[{"key":"billing:read","label_key":"l","description_key":"d","platform_only":false}]
            }}"#,
        );
        assert_eq!(
            judge_declaration(&mut registry, payload),
            DeclarationOutcome::Rejected {
                identity: RejectedIdentity::Service(ServiceKey::new("notifier").unwrap()),
                reason: ScopeDeclarationError::ScopePrefixMismatch {
                    scope_service: "billing".to_string(),
                    declaring_service: "notifier".to_string(),
                }
            }
        );
    }

    #[test]
    fn duplicate_scope_in_declaration_is_rejected() {
        let mut registry = ScopeRegistry::new();
        let payload = raw_declare(
            r#"{"declaration":{
                "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
                "scopes":[
                    {"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false},
                    {"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false}
                ]
            }}"#,
        );
        assert_eq!(
            judge_declaration(&mut registry, payload),
            DeclarationOutcome::Rejected {
                identity: RejectedIdentity::Service(ServiceKey::new("notifier").unwrap()),
                reason: ScopeDeclarationError::DuplicateScopeInDeclaration {
                    key: "notifier:read".to_string(),
                }
            }
        );
    }

    #[test]
    fn malformed_manifest_key_is_rejected_with_an_unrepresentable_identity() {
        let mut registry = ScopeRegistry::new();
        let payload = raw_declare(
            r#"{"declaration":{
                "manifest":{"key":"NOPE","label_key":"l","description_key":"d"},
                "scopes":[]
            }}"#,
        );
        assert_eq!(
            judge_declaration(&mut registry, payload),
            DeclarationOutcome::Rejected {
                identity: RejectedIdentity::Unrepresentable {
                    raw: "NOPE".to_string(),
                },
                reason: ScopeDeclarationError::InvalidScopeKey {
                    key: "NOPE".to_string(),
                    validation: KeyValidationError::InvalidCharset,
                },
            }
        );
    }
}
