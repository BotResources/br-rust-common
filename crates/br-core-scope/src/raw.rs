//! The **receiver-side raw form** of a declaration: [`RawScopeDeclaration`] and
//! its pieces, the unvalidated wire shape Identity deserializes and then
//! [`validate`](RawScopeDeclaration::validate)s.
//!
//! ## Why a raw form exists (the handshake protocol)
//!
//! A declarant must NEVER be left re-publishing forever. If the receiver
//! deserialized straight into the validated [`ScopeDeclaration`], a single
//! malformed key would fail *closed* with an opaque serde error — the receiver
//! could not even read the declaration to reply, so it would nak/redeliver and
//! the declarant would never get a verdict. The protocol forbids that loop: a
//! structurally well-formed declaration the receiver can read MUST be answered
//! with a correlated [`ServiceScopesRejected`](crate::ServiceScopesRejected)
//! carrying a structured [`ScopeDeclarationError`]. So the receiver deserializes
//! into this **raw** form (keys are plain `String`s, not yet validated) and calls
//! [`validate`](RawScopeDeclaration::validate) — the explicit step that produces
//! the structured reason, including a genuinely-raised
//! [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey) on a malformed key.
//!
//! ## A boundary artifact, not a declaration
//!
//! A `RawScopeDeclaration` is **not** a [`ScopeDeclaration`] and cannot be used as
//! one: it exposes only its raw parts, and the single path to a validated
//! [`ScopeDeclaration`] is [`validate`](RawScopeDeclaration::validate). Senders
//! never touch it — they build a validated [`ScopeDeclaration`] and the message
//! constructor lowers it via [`From`], so a well-behaved declarant can never put
//! an invalid declaration on the wire.

use serde::{Deserialize, Serialize};

use crate::declaration::ScopeDeclaration;
use crate::error::ScopeDeclarationError;
use crate::key::ScopeKey;
use crate::service::ServiceKey;
use crate::spec::{ScopeSpec, ServiceManifest};

/// The unvalidated wire shape of a [`ScopeDeclaration`]: a raw manifest plus the
/// raw scopes, with every key still a plain `String`.
///
/// Its serialized shape is **byte-identical** to [`ScopeDeclaration`]'s (and
/// each piece mirrors its validated counterpart), so the sender path (validated
/// → [`From`] → raw → wire) and the receiver path (wire → raw → validate)
/// exchange exactly the same JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScopeDeclaration {
    /// The raw declaring-service manifest.
    pub manifest: RawServiceManifest,
    /// The raw declared scopes.
    pub scopes: Vec<RawScopeSpec>,
}

/// The unvalidated wire shape of a [`ServiceManifest`]: its key as a plain
/// `String`, plus the (already-plain) i18n keys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawServiceManifest {
    /// The declaring service's key, not yet validated as a [`ServiceKey`].
    pub key: String,
    /// i18n key for the service's display label.
    pub label_key: String,
    /// i18n key for the service's description.
    pub description_key: String,
}

/// The unvalidated wire shape of a [`ScopeSpec`]: its key as a plain `String`,
/// plus the (already-plain) display metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RawScopeSpec {
    /// The scope key, not yet validated as a [`ScopeKey`].
    pub key: String,
    /// i18n key for the scope's short label.
    pub label_key: String,
    /// i18n key for the scope's longer description.
    pub description_key: String,
    /// Whether the scope is reserved for platform-internal use.
    pub platform_only: bool,
}

impl RawScopeDeclaration {
    /// Validate this raw declaration into a [`ScopeDeclaration`], or return the
    /// single structured reason it was rejected for — the receiver's explicit
    /// validation step.
    ///
    /// Rejection is atomic: the first failing rule, in field order, wins. Key
    /// syntax is checked first (so a malformed key surfaces as the precise
    /// [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey), not an opaque
    /// serde error), then the cross-cutting rules are delegated to
    /// [`ScopeDeclaration::new`].
    ///
    /// # Errors
    ///
    /// - [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey) if the
    ///   manifest's service key or any scope key fails intrinsic syntax
    ///   validation — it carries the offending key string and the precise
    ///   [`KeyValidationError`](crate::KeyValidationError). (There is no separate
    ///   "invalid service key" reason: a malformed manifest key surfaces under
    ///   this same `invalid_scope_key` reason, as the only structured key-syntax
    ///   reason in the handshake language.)
    /// - [`ScopePrefixMismatch`](ScopeDeclarationError::ScopePrefixMismatch) if a
    ///   scope's service segment differs from the manifest's service key;
    /// - [`DuplicateScopeInDeclaration`](ScopeDeclarationError::DuplicateScopeInDeclaration)
    ///   if a scope key repeats.
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

        // Cross-cutting rules (prefix ownership, no duplicates) live in one place.
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

    // Given a raw form built from a valid declaration → validate() reproduces it.
    #[test]
    fn validate_round_trips_a_valid_declaration() {
        let decl = valid_declaration();
        let raw = RawScopeDeclaration::from(&decl);
        let revalidated = raw.validate().unwrap();
        assert_eq!(decl, revalidated);
    }

    // The sender path (validated → raw → JSON) and the receiver path (the bare
    // ScopeDeclaration → JSON) are BYTE-IDENTICAL: the wire shape is unchanged.
    #[test]
    fn raw_json_is_byte_identical_to_validated() {
        let decl = valid_declaration();
        let from_validated = serde_json::to_string(&decl).unwrap();
        let from_raw = serde_json::to_string(&RawScopeDeclaration::from(&decl)).unwrap();
        assert_eq!(from_validated, from_raw);
    }

    // The receiver path: a structurally well-formed payload with a MALFORMED scope
    // key deserializes, and validate() returns the exact structured reason —
    // InvalidScopeKey { key, validation }, the precise `validation` preserved —
    // never an opaque serde error.
    #[test]
    fn validate_yields_structured_invalid_scope_key() {
        // Proven through the wire (deserialize) path for the charset case…
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
        // …and a wrong-shape (no colon) key preserves its distinct reason.
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

    // A malformed manifest service key surfaces under the same `invalid_scope_key`
    // reason (the only structured key-syntax reason in the language), carrying the
    // offending manifest key.
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

    // With every key well-formed, validate() delegates the cross-cutting rules to
    // ScopeDeclaration::new — so a scope owned by another service and a duplicate
    // key surface as their respective reasons. (The rules themselves are spec'd
    // exhaustively in declaration.rs; here we prove validate() reaches them.)
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

    // Key syntax is checked before the cross-cutting rules: a malformed key wins
    // over a (later) prefix mismatch.
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
