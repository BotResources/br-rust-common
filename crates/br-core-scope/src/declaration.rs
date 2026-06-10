//! [`ScopeDeclaration`] — a service's manifest plus the scopes it declares,
//! validated atomically at construction.

use std::collections::HashSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::ScopeDeclarationError;
use crate::spec::{ScopeSpec, ServiceManifest};

/// What a service declares to Identity: its [`ServiceManifest`] and the set of
/// [`ScopeSpec`]s it owns.
///
/// Construction is **atomic and validating** ([`ScopeDeclaration::new`]): the
/// first failing rule rejects the whole declaration, mirroring the
/// all-or-nothing handshake (Identity either accepts the lot or rejects with one
/// reason). The rules checked locally are:
///
/// 1. every scope key's `{service}` segment matches the manifest's service key
///    ([`ScopePrefixMismatch`](ScopeDeclarationError::ScopePrefixMismatch)) —
///    this is the contextual ownership rule, enforced here because only here is
///    the declaring service known;
/// 2. no scope key appears twice
///    ([`DuplicateScopeInDeclaration`](ScopeDeclarationError::DuplicateScopeInDeclaration)).
///
/// (Each individual key's *syntax* was already enforced when its
/// [`ScopeKey`](crate::ScopeKey) was built; a malformed key cannot reach here.
/// The other two rejection reasons share the language but come from elsewhere:
/// [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey) is produced by the
/// explicit validation of the receiver-side raw form
/// ([`RawScopeDeclaration::validate`](crate::RawScopeDeclaration::validate)) and
/// by Identity's registry, never by this constructor;
/// [`ScopeOwnedByAnotherService`](ScopeDeclarationError::ScopeOwnedByAnotherService)
/// only by Identity's registry.)
///
/// The fields are private (accessed via [`manifest`](ScopeDeclaration::manifest)
/// / [`scopes`](ScopeDeclaration::scopes), no public mutation) so a built
/// declaration cannot drift out of its validated state — re-build through
/// [`new`](ScopeDeclaration::new) to change it.
///
/// Deserializing a bare `ScopeDeclaration` re-runs `new` and **fails closed**:
/// a malformed key surfaces as an opaque `serde` error (the embedded
/// [`ScopeKey`](crate::ScopeKey) is strict), and a cross-cutting violation
/// (prefix mismatch, duplicate) surfaces as a `serde` error wrapping the
/// [`ScopeDeclarationError`] `Display` code. This is the *validated* type, so
/// fail-closed is the intended property. A receiver that needs the **structured**
/// rejection reason (to reply [`ServiceScopesRejected`](crate::ServiceScopesRejected))
/// does not deserialize a `ScopeDeclaration` directly — it deserializes the
/// [`DeclareServiceScopes`](crate::DeclareServiceScopes) payload (which carries
/// the raw form) and calls its `validate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopeDeclaration {
    manifest: ServiceManifest,
    scopes: Vec<ScopeSpec>,
}

// Deserialize via the validating constructor: re-validate the cross-cutting
// invariants (prefix ownership, no duplicates) on the way in. Without this, the
// derived impl would happily rebuild a declaration that `new` would reject —
// a fail-open hole. The intermediate mirrors only the wire fields.
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
    /// Validate the manifest + scopes and assemble a [`ScopeDeclaration`], or
    /// reject with the single reason that failed.
    ///
    /// # Errors
    ///
    /// - [`ScopePrefixMismatch`](ScopeDeclarationError::ScopePrefixMismatch) if
    ///   any scope's service segment differs from the manifest's service key;
    /// - [`DuplicateScopeInDeclaration`](ScopeDeclarationError::DuplicateScopeInDeclaration)
    ///   if any scope key repeats.
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

    /// The declaring service's manifest.
    pub fn manifest(&self) -> &ServiceManifest {
        &self.manifest
    }

    /// The declared scopes.
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

    // Given a manifest and scopes all prefixed by its key, with no duplicates →
    // the declaration is built.
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
        // Declaring no scopes is valid (a service may publish just its manifest).
        let decl = ScopeDeclaration::new(manifest("notifier"), vec![]).unwrap();
        assert!(decl.scopes().is_empty());
    }

    // Given a scope whose prefix is another service → rejected as a prefix
    // mismatch (the contextual ownership rule, enforced at declaration level).
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

    // Given the same scope key twice → rejected as a duplicate.
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

    // Atomic: a mismatch is reported even when a later duplicate also exists —
    // the first failing rule rejects the whole declaration.
    #[test]
    fn rejects_atomically_on_first_failure() {
        let err = ScopeDeclaration::new(
            manifest("notifier"),
            vec![spec("billing:read"), spec("notifier:x"), spec("notifier:x")],
        )
        .unwrap_err();
        // The prefix mismatch comes first in the scope list, so it wins.
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

    // Fail-closed on deserialize: a wire payload that violates the cross-cutting
    // invariants (here a scope owned by another service) must NOT rebuild — the
    // derived impl would have, this validating impl does not.
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
