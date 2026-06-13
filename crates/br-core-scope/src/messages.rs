use serde::{Deserialize, Serialize};

use crate::declaration::ScopeDeclaration;
use crate::error::ScopeDeclarationError;
use crate::raw::RawScopeDeclaration;
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
    use crate::spec::{ScopeSpec, ServiceManifest};

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
