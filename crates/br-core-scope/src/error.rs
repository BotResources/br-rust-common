//! Error types for the scope-declaration language.
//!
//! Two distinct errors, on purpose:
//!
//! - [`KeyValidationError`] — why a single key *string* is malformed. Raised by
//!   the intrinsic constructors ([`ScopeKey::new`], [`ServiceKey::new`]). It
//!   also travels on the wire: a [`ScopeDeclarationError::InvalidScopeKey`]
//!   carries one, so it (de)serializes (internally tagged on `validation`).
//! - [`ScopeDeclarationError`] — the shared **rejection-reason language** of the
//!   handshake. It is the payload of [`ServiceScopesRejected`], so it
//!   (de)serializes. [`InvalidScopeKey`](ScopeDeclarationError::InvalidScopeKey),
//!   [`ScopePrefixMismatch`](ScopeDeclarationError::ScopePrefixMismatch) and
//!   [`DuplicateScopeInDeclaration`](ScopeDeclarationError::DuplicateScopeInDeclaration)
//!   are produced by the explicit validation of the receiver-side raw form
//!   ([`RawScopeDeclaration::validate`]) and by Identity's registry — the last
//!   two also by the local [`ScopeDeclaration`] constructor; the remaining
//!   [`ScopeOwnedByAnotherService`](ScopeDeclarationError::ScopeOwnedByAnotherService)
//!   is produced **only** by Identity's registry.
//!
//! Per the codes-not-language rule, the `#[error("…")]` strings are **stable
//! codes** (`invalid_scope_key`, `scope_prefix_mismatch`, …), never UI prose:
//! the human text and its i18n live at the edge.
//!
//! [`ScopeKey::new`]: crate::ScopeKey::new
//! [`ServiceKey::new`]: crate::ServiceKey::new
//! [`ScopeDeclaration`]: crate::ScopeDeclaration
//! [`ServiceScopesRejected`]: crate::ServiceScopesRejected
//! [`RawScopeDeclaration::validate`]: crate::RawScopeDeclaration::validate

use serde::{Deserialize, Serialize};

/// Why a key string failed intrinsic validation.
///
/// Each variant is a precise, structured reason rather than a rendered
/// sentence, so a caller (or a test) can branch on the exact rule that was
/// broken. The receiver-side raw-form validation
/// ([`RawScopeDeclaration::validate`](crate::RawScopeDeclaration::validate))
/// folds this into [`ScopeDeclarationError::InvalidScopeKey`], keeping the
/// offending key and the reason for the rejection payload — so this type travels
/// on the wire too (nested inside a rejection) and (de)serializes. Its serde
/// shape is internally tagged on `validation`
/// (`{ "validation": "too_long", "max": … }`) and locked by a wire-format test.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future rule is additive.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "validation", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KeyValidationError {
    /// The key (or one of its segments) was empty.
    #[error("empty")]
    Empty,
    /// The key exceeded the maximum length.
    #[error("too_long")]
    TooLong {
        /// The maximum allowed length, in bytes.
        max: usize,
        /// The actual length, in bytes.
        actual: usize,
    },
    /// A character outside the `[a-z0-9_]` charset was present (e.g. uppercase,
    /// an accent, or any non-ASCII byte).
    #[error("invalid_charset")]
    InvalidCharset,
    /// A `{service}:{capability}` scope key did not contain exactly one `:`
    /// separating two non-empty segments.
    #[error("malformed_segments")]
    MalformedSegments,
}

/// The shared rejection-reason language of the scope-declaration handshake.
///
/// One declaration is rejected atomically: the first failing rule yields one of
/// these and the whole declaration is refused. The same enum is the payload of
/// [`ServiceScopesRejected`](crate::ServiceScopesRejected), so it must
/// (de)serialize; its serde shape is internally tagged (`{ "reason": "…", … }`)
/// and locked by a wire-format test.
///
/// Where each reason comes from:
///
/// - [`InvalidScopeKey`](Self::InvalidScopeKey) — produced by the explicit
///   validation of the receiver-side raw form
///   ([`RawScopeDeclaration::validate`](crate::RawScopeDeclaration::validate))
///   when a key is syntactically malformed, and by Identity's registry. **Not**
///   by the local [`ScopeDeclaration`] constructor (which only ever sees
///   already-validated [`ScopeKey`](crate::ScopeKey)s, so a malformed key cannot
///   reach it).
/// - [`ScopePrefixMismatch`](Self::ScopePrefixMismatch) and
///   [`DuplicateScopeInDeclaration`](Self::DuplicateScopeInDeclaration) — the
///   cross-cutting rules, produced by the local [`ScopeDeclaration`] constructor
///   (and so also by the raw-form validation and the registry, which delegate to
///   it).
/// - [`ScopeOwnedByAnotherService`](Self::ScopeOwnedByAnotherService) — produced
///   **only** by Identity's registry (the receiver); it belongs to the same
///   language because a `ServiceScopesRejected` reply must be able to carry it.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future rejection reason
/// is an additive change.
///
/// [`ScopeDeclaration`]: crate::ScopeDeclaration
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScopeDeclarationError {
    /// A key was syntactically invalid — either a scope key or (under this same
    /// reason, the only structured key-syntax reason in the language) the
    /// manifest's service key. Carries the offending key string and the precise
    /// validation reason so the edge can render it per locale. The inner field is
    /// named `validation` (not `reason`) to avoid colliding with this enum's own
    /// `reason` tag on the wire. Produced by
    /// [`RawScopeDeclaration::validate`](crate::RawScopeDeclaration::validate)
    /// (the receiver path) and by Identity's registry, never by the local
    /// [`ScopeDeclaration`](crate::ScopeDeclaration) constructor.
    #[error("invalid_scope_key")]
    InvalidScopeKey {
        /// The offending key string, as supplied.
        key: String,
        /// Why it was rejected.
        validation: KeyValidationError,
    },
    /// A scope key's `{service}` segment did not match the manifest's service
    /// key — a service may only declare scopes it owns. Carries both keys.
    #[error("scope_prefix_mismatch")]
    ScopePrefixMismatch {
        /// The service segment found on the scope key.
        scope_service: String,
        /// The declaring service's key it must have matched.
        declaring_service: String,
    },
    /// The same scope key appeared more than once in the declaration.
    #[error("duplicate_scope_in_declaration")]
    DuplicateScopeInDeclaration {
        /// The duplicated key.
        key: String,
    },
    /// The scope key is already owned by a *different* registered service.
    /// Produced only by Identity's registry, never by local construction.
    #[error("scope_owned_by_another_service")]
    ScopeOwnedByAnotherService {
        /// The contested key.
        key: String,
        /// The service that already owns it.
        owner: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    // codes-not-language: the Display string is a stable code, never a sentence.
    #[test]
    fn declaration_error_codes_are_stable_keys() {
        let mismatch = ScopeDeclarationError::ScopePrefixMismatch {
            scope_service: "billing".to_string(),
            declaring_service: "notifier".to_string(),
        };
        assert_eq!(mismatch.to_string(), "scope_prefix_mismatch");

        let invalid = ScopeDeclarationError::InvalidScopeKey {
            key: "BAD".to_string(),
            validation: KeyValidationError::InvalidCharset,
        };
        assert_eq!(invalid.to_string(), "invalid_scope_key");

        let dup = ScopeDeclarationError::DuplicateScopeInDeclaration {
            key: "notifier:read".to_string(),
        };
        assert_eq!(dup.to_string(), "duplicate_scope_in_declaration");

        let owned = ScopeDeclarationError::ScopeOwnedByAnotherService {
            key: "notifier:read".to_string(),
            owner: "other".to_string(),
        };
        assert_eq!(owned.to_string(), "scope_owned_by_another_service");
    }

    #[test]
    fn key_validation_error_codes_are_stable_keys() {
        assert_eq!(KeyValidationError::Empty.to_string(), "empty");
        assert_eq!(
            KeyValidationError::InvalidCharset.to_string(),
            "invalid_charset"
        );
        assert_eq!(
            KeyValidationError::MalformedSegments.to_string(),
            "malformed_segments"
        );
        assert_eq!(
            KeyValidationError::TooLong {
                max: 128,
                actual: 200
            }
            .to_string(),
            "too_long"
        );
    }

    // The rejection reason is the `ServiceScopesRejected` payload, so every
    // variant must (de)serialize. Shape is internally tagged on `reason`.
    #[test]
    fn declaration_error_serde_roundtrip_all_variants() {
        let variants = [
            ScopeDeclarationError::InvalidScopeKey {
                key: "bad key".to_string(),
                validation: KeyValidationError::InvalidCharset,
            },
            ScopeDeclarationError::ScopePrefixMismatch {
                scope_service: "billing".to_string(),
                declaring_service: "notifier".to_string(),
            },
            ScopeDeclarationError::DuplicateScopeInDeclaration {
                key: "notifier:read".to_string(),
            },
            ScopeDeclarationError::ScopeOwnedByAnotherService {
                key: "notifier:read".to_string(),
                owner: "other".to_string(),
            },
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: ScopeDeclarationError = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    // KeyValidationError travels nested inside a rejection; lock its tagged shape.
    #[test]
    fn key_validation_error_wire_shape_is_tagged_on_validation() {
        let charset = serde_json::to_value(KeyValidationError::InvalidCharset).unwrap();
        assert_eq!(charset["validation"], "invalid_charset");

        let too_long = serde_json::to_value(KeyValidationError::TooLong {
            max: 128,
            actual: 200,
        })
        .unwrap();
        assert_eq!(too_long["validation"], "too_long");
        assert_eq!(too_long["max"], 128);
        assert_eq!(too_long["actual"], 200);
    }

    #[test]
    fn key_validation_error_serde_roundtrip_all_variants() {
        let variants = [
            KeyValidationError::Empty,
            KeyValidationError::InvalidCharset,
            KeyValidationError::MalformedSegments,
            KeyValidationError::TooLong {
                max: 128,
                actual: 200,
            },
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: KeyValidationError = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn declaration_error_wire_shape_is_tagged_on_reason() {
        let err = ScopeDeclarationError::ScopePrefixMismatch {
            scope_service: "billing".to_string(),
            declaring_service: "notifier".to_string(),
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["reason"], "scope_prefix_mismatch");
        assert_eq!(json["scope_service"], "billing");
        assert_eq!(json["declaring_service"], "notifier");
    }
}
