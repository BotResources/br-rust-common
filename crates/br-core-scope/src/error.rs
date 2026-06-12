use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "validation", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KeyValidationError {
    #[error("empty")]
    Empty,
    #[error("too_long")]
    TooLong {
        max: usize,
        actual: usize,
    },
    #[error("invalid_charset")]
    InvalidCharset,
    #[error("malformed_segments")]
    MalformedSegments,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScopeDeclarationError {
    #[error("invalid_scope_key")]
    InvalidScopeKey {
        key: String,
        validation: KeyValidationError,
    },
    #[error("scope_prefix_mismatch")]
    ScopePrefixMismatch {
        scope_service: String,
        declaring_service: String,
    },
    #[error("duplicate_scope_in_declaration")]
    DuplicateScopeInDeclaration {
        key: String,
    },
    #[error("scope_owned_by_another_service")]
    ScopeOwnedByAnotherService {
        key: String,
        owner: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

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
