//! [`judge_declaration`] — the pure receiver-side decision function for a
//! scope-declaration.
//!
//! It is the one call the application layer makes between `load` and `save`:
//! given the loaded [`ScopeRegistry`] and the received
//! [`DeclareServiceScopes`](br_core_scope::DeclareServiceScopes) payload, it
//! produces the **whole** accepted/rejected verdict — no I/O, no transport.
//!
//! It composes the two halves of the rejection language:
//!
//! - the **boundary validation** ([`DeclareServiceScopes::validate`]) — key
//!   syntax, prefix ownership, intra-declaration duplicates (the three reasons
//!   `br-core-scope` produces:
//!   [`InvalidScopeKey`](br_core_scope::ScopeDeclarationError::InvalidScopeKey),
//!   [`ScopePrefixMismatch`](br_core_scope::ScopeDeclarationError::ScopePrefixMismatch),
//!   [`DuplicateScopeInDeclaration`](br_core_scope::ScopeDeclarationError::DuplicateScopeInDeclaration));
//! - the **registry judgment** ([`ScopeRegistry::register_declaration`]) — the
//!   cross-service
//!   [`ScopeOwnedByAnotherService`](br_core_scope::ScopeDeclarationError::ScopeOwnedByAnotherService)
//!   conflict and idempotent re-declaration.
//!
//! The result is one pure function producing the full
//! [`ScopeDeclarationError`](br_core_scope::ScopeDeclarationError) language, so
//! the application layer only marshals I/O around it.

use br_core_scope::{DeclareServiceScopes, ScopeDeclarationError, ServiceKey};

use crate::event::CommandResult;
use crate::registry::ScopeRegistry;

/// The verdict of [`judge_declaration`].
///
/// On [`Accepted`](DeclarationOutcome::Accepted) the application layer persists
/// the registry, dispatches the [`events`](CommandResult::events), and replies
/// `ServiceScopesAccepted { service }`. On
/// [`Rejected`](DeclarationOutcome::Rejected) it persists nothing and replies
/// `ServiceScopesRejected { service, reason }` (it sources the reply's `service`
/// from the original payload, since a rejection may itself be *about* a
/// malformed service key).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeclarationOutcome {
    /// The declaration was accepted. Carries the (validated) declaring service
    /// and the command's result; the result is empty on an idempotent no-op.
    Accepted {
        /// The validated declaring service key — the `ServiceScopesAccepted`
        /// reply's `service`.
        service: ServiceKey,
        /// The events to dispatch (empty on an idempotent re-declaration).
        result: CommandResult,
    },
    /// The declaration was rejected, atomically, for this single reason — the
    /// `ServiceScopesRejected` reply's `reason`.
    Rejected {
        /// Why the declaration was refused.
        reason: ScopeDeclarationError,
    },
}

/// Judge a received declaration against `registry`, producing the full verdict.
///
/// Pure: on [`Accepted`](DeclarationOutcome::Accepted) the `registry` has been
/// mutated in-memory (and its [`version`](ScopeRegistry::version) bumped unless
/// the call was an idempotent no-op); on
/// [`Rejected`](DeclarationOutcome::Rejected) the `registry` is **untouched** —
/// validation fails before the command runs, and the command itself rejects
/// before any mutation, so a refusal never leaves a partial registration.
///
/// The application layer calls this between `load` and `save`: persist on
/// `Accepted`, persist nothing on `Rejected`, and in both cases emit the
/// correlated integration reply.
pub fn judge_declaration(
    registry: &mut ScopeRegistry,
    command: DeclareServiceScopes,
) -> DeclarationOutcome {
    let declaration = match command.validate() {
        Ok(declaration) => declaration,
        Err(reason) => return DeclarationOutcome::Rejected { reason },
    };
    let service = declaration.manifest().key.clone();
    match registry.register_declaration(&declaration) {
        Ok(result) => DeclarationOutcome::Accepted { service, result },
        Err(reason) => DeclarationOutcome::Rejected { reason },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_scope::{
        KeyValidationError, ScopeDeclaration, ScopeKey, ScopeSpec, ServiceManifest,
    };

    use crate::event::RegistryEvent;

    // The sender's validated path → a DeclareServiceScopes that judges as accepted.
    fn declare(service: &str, scope_keys: &[&str]) -> DeclareServiceScopes {
        let scopes = scope_keys
            .iter()
            .map(|k| ScopeSpec::new(ScopeKey::new(*k).unwrap(), "l", "d", false))
            .collect();
        let manifest = ServiceManifest::new(ServiceKey::new(service).unwrap(), "l", "d");
        DeclareServiceScopes::new(ScopeDeclaration::new(manifest, scopes).unwrap())
    }

    // The receiver's raw path: a structurally well-formed payload that may carry
    // invalid content (a malformed key, a prefix mismatch, a duplicate), built by
    // deserializing exactly as the application layer will off the bus.
    fn raw_declare(json: &str) -> DeclareServiceScopes {
        serde_json::from_str(json).unwrap()
    }

    // ─── accepted path ─────────────────────────────────

    // Given an empty registry, When a valid declaration is judged, Then the
    // outcome is Accepted carrying the declaring service and the command events,
    // and the registry is mutated.
    #[test]
    fn valid_declaration_is_accepted_and_mutates_the_registry() {
        let mut registry = ScopeRegistry::new();
        let outcome = judge_declaration(&mut registry, declare("notifier", &["notifier:read"]));
        match outcome {
            DeclarationOutcome::Accepted { service, result } => {
                assert_eq!(service, ServiceKey::new("notifier").unwrap());
                assert_eq!(result.events.len(), 2); // ServiceRegistered + ScopeRegistered
                assert!(matches!(
                    result.events[1],
                    RegistryEvent::ScopeRegistered { .. }
                ));
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
        assert_eq!(registry.version(), 1);
    }

    // An idempotent re-declaration is Accepted with an empty (no-op) result.
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

    // ─── rejected path: the boundary-validation reasons ─

    // A malformed scope key is rejected with the structured InvalidScopeKey reason
    // (the receiver path), and the registry is left untouched.
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
                reason: ScopeDeclarationError::InvalidScopeKey {
                    key: "notifier:BAD".to_string(),
                    validation: KeyValidationError::InvalidCharset,
                }
            }
        );
        assert_eq!(registry, before, "a rejection must not touch the registry");
    }

    // A scope owned by another service (prefix mismatch) is rejected.
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
                reason: ScopeDeclarationError::ScopePrefixMismatch {
                    scope_service: "billing".to_string(),
                    declaring_service: "notifier".to_string(),
                }
            }
        );
    }

    // A duplicate scope key within the declaration is rejected.
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
                reason: ScopeDeclarationError::DuplicateScopeInDeclaration {
                    key: "notifier:read".to_string(),
                }
            }
        );
    }

    // A malformed manifest service key is rejected (surfaces under the single
    // structured key-syntax reason, InvalidScopeKey).
    #[test]
    fn malformed_manifest_key_is_rejected() {
        let mut registry = ScopeRegistry::new();
        let payload = raw_declare(
            r#"{"declaration":{
                "manifest":{"key":"NOPE","label_key":"l","description_key":"d"},
                "scopes":[]
            }}"#,
        );
        assert!(matches!(
            judge_declaration(&mut registry, payload),
            DeclarationOutcome::Rejected {
                reason: ScopeDeclarationError::InvalidScopeKey { .. }
            }
        ));
    }
}
