use serde::{Deserialize, Serialize};

use crate::declaration::{ScopeDeclaration, ScopeDeclarationV2};
use crate::error::ScopeDeclarationError;
use crate::raw::{RawScopeDeclaration, RawScopeDeclarationV2};
use crate::service::ServiceKey;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclareServiceScopes {
    declaration: RawScopeDeclaration,
}

impl DeclareServiceScopes {
    pub fn new(declaration: ScopeDeclaration) -> Self {
        Self {
            declaration: RawScopeDeclaration::from(declaration),
        }
    }

    pub fn validate(self) -> Result<ScopeDeclaration, ScopeDeclarationError> {
        self.declaration.validate()
    }

    pub fn raw(&self) -> &RawScopeDeclaration {
        &self.declaration
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclareServiceScopesV2 {
    declaration: RawScopeDeclarationV2,
}

impl DeclareServiceScopesV2 {
    pub fn new(declaration: ScopeDeclarationV2) -> Self {
        Self {
            declaration: RawScopeDeclarationV2::from(declaration),
        }
    }

    pub fn validate(self) -> Result<ScopeDeclarationV2, ScopeDeclarationError> {
        self.declaration.validate()
    }

    pub fn raw(&self) -> &RawScopeDeclarationV2 {
        &self.declaration
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceScopesAccepted {
    pub service: ServiceKey,
}

impl ServiceScopesAccepted {
    pub fn new(service: ServiceKey) -> Self {
        Self { service }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceScopesRejected {
    pub service: ServiceKey,
    pub reason: ScopeDeclarationError,
}

impl ServiceScopesRejected {
    pub fn new(service: ServiceKey, reason: ScopeDeclarationError) -> Self {
        Self { service, reason }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::KeyValidationError;
    use crate::key::ScopeKey;
    use crate::spec::{ScopeSpec, ScopeSpecV2, ServiceManifest};

    fn declaration() -> ScopeDeclaration {
        ScopeDeclaration::new(
            ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "l", "d"),
            vec![ScopeSpec::new(
                ScopeKey::new("notifier:read").unwrap(),
                "l",
                "d",
                false,
            )],
        )
        .unwrap()
    }

    #[test]
    fn declare_payload_roundtrip() {
        let cmd = DeclareServiceScopes::new(declaration());
        let json = serde_json::to_string(&cmd).unwrap();
        let back: DeclareServiceScopes = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
    }

    #[test]
    fn sender_and_receiver_paths_share_wire_and_declaration() {
        let decl = declaration();
        let cmd = DeclareServiceScopes::new(decl.clone());
        let json = serde_json::to_string(&cmd).unwrap();

        let received: DeclareServiceScopes = serde_json::from_str(&json).unwrap();
        assert_eq!(received.validate().unwrap(), decl);
    }

    #[test]
    fn validate_yields_structured_reason_on_malformed_key() {
        let json = r#"{"declaration":{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"notifier:BAD","label_key":"l","description_key":"d","platform_only":false}]
        }}"#;
        let cmd: DeclareServiceScopes = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd.validate().unwrap_err(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "notifier:BAD".to_string(),
                validation: KeyValidationError::InvalidCharset,
            }
        );
    }

    #[test]
    fn declare_v1_wire_bytes_are_unchanged() {
        let json = serde_json::to_string(&DeclareServiceScopes::new(declaration())).unwrap();
        assert_eq!(
            json,
            r#"{"declaration":{"manifest":{"key":"notifier","label_key":"l","description_key":"d"},"scopes":[{"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false}]}}"#
        );
    }

    fn declaration_v2() -> ScopeDeclarationV2 {
        ScopeDeclarationV2::new(
            ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "l", "d"),
            vec![
                ScopeSpecV2::new(ScopeKey::new("notifier:read").unwrap(), "l", "d", false),
                ScopeSpecV2::new(ScopeKey::new("notifier:admin").unwrap(), "l", "d", false)
                    .with_implies([ScopeKey::new("notifier:read").unwrap()])
                    .with_service_accounts_only(true),
            ],
        )
        .unwrap()
    }

    #[test]
    fn declare_v2_payload_roundtrip() {
        let cmd = DeclareServiceScopesV2::new(declaration_v2());
        let json = serde_json::to_string(&cmd).unwrap();
        let back: DeclareServiceScopesV2 = serde_json::from_str(&json).unwrap();
        assert_eq!(cmd, back);
        assert_eq!(back.validate().unwrap(), declaration_v2());
    }

    #[test]
    fn declare_v2_wire_shape() {
        let json = serde_json::to_string(&DeclareServiceScopesV2::new(declaration_v2())).unwrap();
        assert_eq!(
            json,
            concat!(
                r#"{"declaration":{"manifest":{"key":"notifier","label_key":"l","description_key":"d"},"scopes":["#,
                r#"{"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false,"implies":[],"service_accounts_only":false},"#,
                r#"{"key":"notifier:admin","label_key":"l","description_key":"d","platform_only":false,"implies":["notifier:read"],"service_accounts_only":true}"#,
                r#"]}}"#
            )
        );
    }

    #[test]
    fn declare_v2_minimal_payload_parses_with_defaults() {
        let json = r#"{"declaration":{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"notifier:read","label_key":"l","description_key":"d","platform_only":false}]
        }}"#;
        let cmd: DeclareServiceScopesV2 = serde_json::from_str(json).unwrap();
        let decl = cmd.validate().unwrap();
        assert!(decl.scopes()[0].implies.is_empty());
        assert!(!decl.scopes()[0].service_accounts_only);
    }

    #[test]
    fn a_v1_payload_reads_as_v2_with_defaults() {
        let v1 = serde_json::to_string(&DeclareServiceScopes::new(declaration())).unwrap();
        let as_v2: DeclareServiceScopesV2 = serde_json::from_str(&v1).unwrap();
        assert_eq!(
            as_v2.validate().unwrap(),
            ScopeDeclarationV2::from(declaration())
        );
    }

    #[test]
    fn declare_v2_validate_yields_structured_implication_reason() {
        let json = r#"{"declaration":{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"notifier:admin","label_key":"l","description_key":"d","platform_only":false,"implies":["notifier:ghost"]}]
        }}"#;
        let cmd: DeclareServiceScopesV2 = serde_json::from_str(json).unwrap();
        assert_eq!(
            cmd.validate().unwrap_err(),
            ScopeDeclarationError::UnknownImpliedScope {
                key: "notifier:admin".to_string(),
                implied: "notifier:ghost".to_string(),
            }
        );
    }

    #[test]
    fn rejected_payload_carries_an_implication_reason() {
        let evt = ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            ScopeDeclarationError::ImplicationCycle {
                cycle: vec!["notifier:a".to_string(), "notifier:a".to_string()],
            },
        );
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["reason"]["reason"], "implication_cycle");
        let back: ServiceScopesRejected = serde_json::from_value(json).unwrap();
        assert_eq!(evt, back);
    }

    #[test]
    fn accepted_payload_roundtrip_and_shape() {
        let evt = ServiceScopesAccepted::new(ServiceKey::new("notifier").unwrap());
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["service"], "notifier");
        let back: ServiceScopesAccepted = serde_json::from_value(json).unwrap();
        assert_eq!(evt, back);
    }

    #[test]
    fn rejected_payload_roundtrip() {
        let evt = ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            ScopeDeclarationError::ScopeOwnedByAnotherService {
                key: "notifier:read".to_string(),
                owner: "billing".to_string(),
            },
        );
        let json = serde_json::to_string(&evt).unwrap();
        let back: ServiceScopesRejected = serde_json::from_str(&json).unwrap();
        assert_eq!(evt, back);
    }

    #[test]
    fn rejected_payload_wire_shape() {
        let evt = ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            ScopeDeclarationError::InvalidScopeKey {
                key: "BAD".to_string(),
                validation: KeyValidationError::InvalidCharset,
            },
        );
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["service"], "notifier");
        assert_eq!(json["reason"]["reason"], "invalid_scope_key");
        assert_eq!(json["reason"]["key"], "BAD");
        assert_eq!(
            json["reason"]["validation"]["validation"],
            "invalid_charset"
        );
    }

    #[test]
    fn rejected_payload_carries_registry_only_reason() {
        let evt = ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            ScopeDeclarationError::ScopeOwnedByAnotherService {
                key: "notifier:read".to_string(),
                owner: "billing".to_string(),
            },
        );
        let json = serde_json::to_value(&evt).unwrap();
        assert_eq!(json["reason"]["reason"], "scope_owned_by_another_service");
        assert_eq!(json["reason"]["owner"], "billing");
    }
}
