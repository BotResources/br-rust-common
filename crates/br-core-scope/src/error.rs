use serde::{Deserialize, Serialize};

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "validation", rename_all = "snake_case")]
#[non_exhaustive]
pub enum KeyValidationError {
    #[error("empty")]
    Empty,
    #[error("too_long")]
    TooLong { max: usize, actual: usize },
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
    DuplicateScopeInDeclaration { key: String },
    #[error("scope_owned_by_another_service")]
    ScopeOwnedByAnotherService { key: String, owner: String },
    #[error("unknown_implied_scope")]
    UnknownImpliedScope { key: String, implied: String },
    #[error("implied_scope_prefix_mismatch")]
    ImpliedScopePrefixMismatch {
        key: String,
        implied: String,
        declaring_service: String,
    },
    #[error("implication_cycle")]
    ImplicationCycle { cycle: Vec<String> },
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
    fn implication_error_codes_are_stable_keys() {
        let unknown = ScopeDeclarationError::UnknownImpliedScope {
            key: "notifier:admin".to_string(),
            implied: "notifier:ghost".to_string(),
        };
        assert_eq!(unknown.to_string(), "unknown_implied_scope");

        let foreign = ScopeDeclarationError::ImpliedScopePrefixMismatch {
            key: "notifier:admin".to_string(),
            implied: "billing:read".to_string(),
            declaring_service: "notifier".to_string(),
        };
        assert_eq!(foreign.to_string(), "implied_scope_prefix_mismatch");

        let cycle = ScopeDeclarationError::ImplicationCycle {
            cycle: vec!["notifier:a".to_string(), "notifier:a".to_string()],
        };
        assert_eq!(cycle.to_string(), "implication_cycle");
    }

    #[test]
    fn implication_error_wire_shapes_are_tagged_on_reason() {
        let unknown = serde_json::to_value(ScopeDeclarationError::UnknownImpliedScope {
            key: "notifier:admin".to_string(),
            implied: "notifier:ghost".to_string(),
        })
        .unwrap();
        assert_eq!(unknown["reason"], "unknown_implied_scope");
        assert_eq!(unknown["key"], "notifier:admin");
        assert_eq!(unknown["implied"], "notifier:ghost");

        let foreign = serde_json::to_value(ScopeDeclarationError::ImpliedScopePrefixMismatch {
            key: "notifier:admin".to_string(),
            implied: "billing:read".to_string(),
            declaring_service: "notifier".to_string(),
        })
        .unwrap();
        assert_eq!(foreign["reason"], "implied_scope_prefix_mismatch");
        assert_eq!(foreign["implied"], "billing:read");
        assert_eq!(foreign["declaring_service"], "notifier");

        let cycle = serde_json::to_value(ScopeDeclarationError::ImplicationCycle {
            cycle: vec![
                "notifier:a".to_string(),
                "notifier:b".to_string(),
                "notifier:a".to_string(),
            ],
        })
        .unwrap();
        assert_eq!(cycle["reason"], "implication_cycle");
        assert_eq!(
            cycle["cycle"],
            serde_json::json!(["notifier:a", "notifier:b", "notifier:a"])
        );
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
            ScopeDeclarationError::UnknownImpliedScope {
                key: "notifier:admin".to_string(),
                implied: "notifier:ghost".to_string(),
            },
            ScopeDeclarationError::ImpliedScopePrefixMismatch {
                key: "notifier:admin".to_string(),
                implied: "billing:read".to_string(),
                declaring_service: "notifier".to_string(),
            },
            ScopeDeclarationError::ImplicationCycle {
                cycle: vec!["notifier:a".to_string(), "notifier:a".to_string()],
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
