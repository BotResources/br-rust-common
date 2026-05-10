use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// How a `Passport::Human` credential was authenticated.
///
/// Carried on `Passport::Human` so downstream services can branch on the
/// auth path — e.g. AI assistants typically act with a user's PAT, and some
/// services may want to deny certain operations or apply tighter RLS on PAT
/// traffic.
///
/// `Passport::Service` has no `AuthMethod`: the variant itself is the
/// auth signal (machine identity via API key).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Cookie-bound JWT (the SPA / browser-session path).
    Jwt,
    /// Personal Access Token. `token_id` identifies the specific PAT and
    /// is the handle used for audit logs and revocation.
    Pat { token_id: Uuid },
}

impl AuthMethod {
    /// Returns `true` if authenticated via PAT.
    pub fn is_pat(&self) -> bool {
        matches!(self, AuthMethod::Pat { .. })
    }

    /// Returns the PAT's `token_id` if this is a PAT, else `None`.
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
}
