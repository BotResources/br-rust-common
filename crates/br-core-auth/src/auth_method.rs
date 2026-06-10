use serde::{Deserialize, Deserializer, Serialize};
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
///
/// Deserialization is **strict**: an unknown field on either variant's
/// payload (including the unit `Jwt` payload) is rejected. This crate is a
/// security DTO shared by every service, so a contract mismatch must fail
/// loud rather than silently swallow extra fields (see the `AuthMethodWire`
/// deserialization mirror below).
// SYNC: any variant added here MUST also be added to `AuthMethodWire` below.
// The compile-time guard is `wire_mirror_is_complete` in the test module: its
// exhaustive match stops compiling if the two enums drift apart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum AuthMethod {
    /// Cookie-bound JWT (the SPA / browser-session path).
    Jwt,
    /// Personal Access Token. `token_id` identifies the specific PAT and
    /// is the handle used for audit logs and revocation.
    Pat { token_id: Uuid },
}

/// Private wire mirror of [`AuthMethod`] used only for deserialization.
///
/// serde's `deny_unknown_fields` is a no-op on a *unit* variant (there is no
/// field set to deny against), so `AuthMethod::Jwt` modeled as a unit variant
/// would silently accept `{"method":"jwt","evil":…}`. Modeling `Jwt` here as
/// an empty-struct variant `Jwt {}` gives it a (zero-sized) field set, so
/// `deny_unknown_fields` rejects extras on every variant. The public API keeps
/// the unit `Jwt`, and the wire format is byte-identical (`{"method":"jwt"}`).
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

    // ─── strict deserialization (unknown fields) ──────────

    // Given a JWT (unit) payload carrying an extra field
    // When deserializing
    // Then it is rejected — `deny_unknown_fields` is enforced even on the
    // unit variant via the empty-struct wire mirror
    #[test]
    fn deserialize_rejects_unknown_field_on_jwt() {
        let json = r#"{"method":"jwt","evil":true}"#;
        assert!(serde_json::from_str::<AuthMethod>(json).is_err());
    }

    // Given a PAT payload carrying an extra field
    // When deserializing
    // Then it is rejected
    #[test]
    fn deserialize_rejects_unknown_field_on_pat() {
        let json =
            r#"{"method":"pat","token_id":"00000000-0000-0000-0000-000000000001","evil":true}"#;
        assert!(serde_json::from_str::<AuthMethod>(json).is_err());
    }

    // Given the canonical JWT wire shape `{"method":"jwt"}`
    // When deserializing
    // Then it is still accepted — strictness does not change the valid format
    #[test]
    fn deserialize_accepts_canonical_jwt_shape() {
        let m: AuthMethod = serde_json::from_str(r#"{"method":"jwt"}"#).unwrap();
        assert_eq!(m, AuthMethod::Jwt);
    }

    // Given a JWT value serialized by this crate
    // When inspecting the wire bytes
    // Then they are byte-identical to the pre-strictness format
    #[test]
    fn jwt_wire_format_unchanged() {
        assert_eq!(
            serde_json::to_string(&AuthMethod::Jwt).unwrap(),
            r#"{"method":"jwt"}"#
        );
    }

    // Compile-time drift guard: `Serialize` is derived on the public enum while
    // `Deserialize` goes through the private wire mirror, so a variant added to
    // `AuthMethod` alone would serialize but silently fail to deserialize. The
    // exhaustive match below stops compiling the moment the two enums diverge.
    #[test]
    fn wire_mirror_is_complete() {
        fn mirror(m: AuthMethod) -> AuthMethodWire {
            match m {
                AuthMethod::Jwt => AuthMethodWire::Jwt {},
                AuthMethod::Pat { token_id } => AuthMethodWire::Pat { token_id },
            }
        }

        // Behavioral half of the guard: every public variant round-trips.
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
