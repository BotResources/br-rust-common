use base64::Engine;

use crate::error::PassportError;
use crate::passport::Passport;

pub trait PassportHeader: Sized {
    fn to_header(&self) -> String;

    fn from_header(header: &str) -> Result<Self, PassportError>;
}

impl PassportHeader for Passport {
    fn to_header(&self) -> String {
        let json = serde_json::to_string(self).expect("Passport serialization cannot fail");
        base64::engine::general_purpose::STANDARD.encode(json.as_bytes())
    }

    fn from_header(header: &str) -> Result<Self, PassportError> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(header)
            .map_err(|e| PassportError::Malformed(format!("invalid base64 in X-Passport: {e}")))?;

        serde_json::from_slice(&bytes)
            .map_err(|e| PassportError::Malformed(format!("invalid JSON in X-Passport: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use uuid::Uuid;

    fn make_human() -> Passport {
        Passport::Human {
            user_id: Uuid::now_v7(),
            is_super_admin: true,
            is_active: true,
            auth_method: crate::AuthMethod::Jwt,
            impersonator: None,
            claims: json!({"email": "test@example.com"}),
        }
    }

    fn make_service() -> Passport {
        Passport::Service {
            service_account_id: Uuid::now_v7(),
            claims: json!({"name": "ci-bot"}),
        }
    }

    #[test]
    fn roundtrip_human() {
        let p = make_human();
        let header = p.to_header();
        let p2 = Passport::from_header(&header).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn roundtrip_service() {
        let p = make_service();
        let header = p.to_header();
        let p2 = Passport::from_header(&header).unwrap();
        assert_eq!(p, p2);
    }

    #[test]
    fn to_header_produces_base64_with_kind_tag() {
        let p = make_human();
        let header = p.to_header();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&header)
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["kind"], "human");
        assert!(v.get("user_id").is_some());
        assert!(v.get("is_super_admin").is_some());
        assert!(v.get("is_active").is_some());
        assert!(v.get("claims").is_some());
    }

    #[test]
    fn from_header_rejects_garbage() {
        let err = Passport::from_header("not-base64!!!").unwrap_err();
        assert!(matches!(err, PassportError::Malformed(_)));
    }

    #[test]
    fn from_header_rejects_valid_base64_invalid_json() {
        let b64 = base64::engine::general_purpose::STANDARD.encode("this is not json");
        let err = Passport::from_header(&b64).unwrap_err();
        assert!(matches!(err, PassportError::Malformed(_)));
    }

    #[test]
    fn from_header_rejects_wrong_json_shape() {
        let b64 = base64::engine::general_purpose::STANDARD.encode(r#"{"foo":"bar"}"#);
        let err = Passport::from_header(&b64).unwrap_err();
        assert!(matches!(err, PassportError::Malformed(_)));
    }

    #[test]
    fn from_header_rejects_missing_kind() {
        let json = r#"{"user_id":"00000000-0000-0000-0000-000000000000","is_super_admin":false,"is_active":true,"claims":{}}"#;
        let b64 = base64::engine::general_purpose::STANDARD.encode(json);
        let err = Passport::from_header(&b64).unwrap_err();
        assert!(matches!(err, PassportError::Malformed(_)));
    }
}
