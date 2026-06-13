use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AuthMethod {
    Jwt,
    Pat { token_id: Uuid },
}

#[derive(Deserialize)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
enum AuthMethodWire {
    Jwt {},
    Pat { token_id: Uuid },
}

impl<'de> Deserialize<'de> for AuthMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(match AuthMethodWire::deserialize(deserializer)? {
            AuthMethodWire::Jwt {} => AuthMethod::Jwt,
            AuthMethodWire::Pat { token_id } => AuthMethod::Pat { token_id },
        })
    }
}

impl AuthMethod {
    pub fn is_pat(&self) -> bool {
        matches!(self, AuthMethod::Pat { .. })
    }

    pub fn pat_token_id(&self) -> Option<Uuid> {
        match self {
            AuthMethod::Pat { token_id } => Some(*token_id),
            AuthMethod::Jwt => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn jwt_serializes_with_method_tag() {
        let v = serde_json::to_value(AuthMethod::Jwt).unwrap();
        assert_eq!(v, json!({"method": "jwt"}));
    }

    #[test]
    fn pat_serializes_with_token_id() {
        let id = Uuid::from_u128(7);
        let v = serde_json::to_value(AuthMethod::Pat { token_id: id }).unwrap();
        assert_eq!(v["method"], "pat");
        assert_eq!(v["token_id"], id.to_string());
    }

    #[test]
    fn jwt_roundtrip() {
        let m = AuthMethod::Jwt;
        let back: AuthMethod = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn pat_roundtrip() {
        let m = AuthMethod::Pat {
            token_id: Uuid::from_u128(42),
        };
        let back: AuthMethod = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(m, back);
    }

    #[test]
    fn is_pat_true_for_pat() {
        assert!(
            AuthMethod::Pat {
                token_id: Uuid::nil()
            }
            .is_pat()
        );
    }

    #[test]
    fn is_pat_false_for_jwt() {
        assert!(!AuthMethod::Jwt.is_pat());
    }

    #[test]
    fn pat_token_id_returns_id() {
        let id = Uuid::from_u128(13);
        assert_eq!(AuthMethod::Pat { token_id: id }.pat_token_id(), Some(id));
    }

    #[test]
    fn pat_token_id_none_for_jwt() {
        assert_eq!(AuthMethod::Jwt.pat_token_id(), None);
    }

    #[test]
    fn deserialize_rejects_unknown_method() {
        let json = r#"{"method":"oauth"}"#;
        assert!(serde_json::from_str::<AuthMethod>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_pat_without_token_id() {
        let json = r#"{"method":"pat"}"#;
        assert!(serde_json::from_str::<AuthMethod>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_field_on_jwt() {
        let json = r#"{"method":"jwt","evil":true}"#;
        assert!(serde_json::from_str::<AuthMethod>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_field_on_pat() {
        let json =
            r#"{"method":"pat","token_id":"00000000-0000-0000-0000-000000000001","evil":true}"#;
        assert!(serde_json::from_str::<AuthMethod>(json).is_err());
    }

    #[test]
    fn deserialize_accepts_canonical_jwt_shape() {
        let m: AuthMethod = serde_json::from_str(r#"{"method":"jwt"}"#).unwrap();
        assert_eq!(m, AuthMethod::Jwt);
    }

    #[test]
    fn jwt_wire_format_unchanged() {
        assert_eq!(
            serde_json::to_string(&AuthMethod::Jwt).unwrap(),
            r#"{"method":"jwt"}"#
        );
    }

    #[test]
    fn wire_mirror_is_complete() {
        fn mirror(m: AuthMethod) -> AuthMethodWire {
            match m {
                AuthMethod::Jwt => AuthMethodWire::Jwt {},
                AuthMethod::Pat { token_id } => AuthMethodWire::Pat { token_id },
            }
        }

        for m in [
            AuthMethod::Jwt,
            AuthMethod::Pat {
                token_id: Uuid::from_u128(9),
            },
        ] {
            let _ = mirror(m.clone());
            let back: AuthMethod =
                serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
            assert_eq!(m, back);
        }
    }
}
