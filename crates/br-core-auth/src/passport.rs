use br_core_kernel::{Actor, ServiceAccountId, UserId};
use br_core_scope::ScopeKey;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use uuid::Uuid;

use crate::auth_method::AuthMethod;

pub const SCOPES_CLAIM_KEY: &str = "scopes";

fn deserialize_claims<'de, D>(deserializer: D) -> Result<serde_json::Value, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_object() {
        Ok(value)
    } else {
        Err(D::Error::custom("claims must be a JSON object"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Passport {
    Human {
        user_id: Uuid,
        is_super_admin: bool,
        is_active: bool,
        auth_method: AuthMethod,
        #[serde(default)]
        impersonator: Option<Uuid>,
        #[serde(deserialize_with = "deserialize_claims")]
        claims: serde_json::Value,
    },
    Service {
        service_account_id: Uuid,
        #[serde(deserialize_with = "deserialize_claims")]
        claims: serde_json::Value,
    },
}

impl Passport {
    pub fn actor_id(&self) -> Uuid {
        match self {
            Passport::Human { user_id, .. } => *user_id,
            Passport::Service {
                service_account_id, ..
            } => *service_account_id,
        }
    }

    pub fn to_actor(&self) -> Actor {
        match self {
            Passport::Human { user_id, .. } => Actor::Human(UserId::from(*user_id)),
            Passport::Service {
                service_account_id, ..
            } => Actor::Service(ServiceAccountId::from(*service_account_id)),
        }
    }

    pub fn is_super_admin(&self) -> bool {
        matches!(
            self,
            Passport::Human {
                is_super_admin: true,
                ..
            }
        )
    }

    pub fn is_active(&self) -> bool {
        match self {
            Passport::Human { is_active, .. } => *is_active,
            Passport::Service { .. } => true,
        }
    }

    pub fn auth_method(&self) -> Option<&AuthMethod> {
        match self {
            Passport::Human { auth_method, .. } => Some(auth_method),
            Passport::Service { .. } => None,
        }
    }

    pub fn is_pat(&self) -> bool {
        matches!(self.auth_method(), Some(m) if m.is_pat())
    }

    pub fn is_impersonating(&self) -> bool {
        self.impersonator_id().is_some()
    }

    pub fn impersonator_id(&self) -> Option<Uuid> {
        match self {
            Passport::Human { impersonator, .. } => *impersonator,
            Passport::Service { .. } => None,
        }
    }

    pub fn claims(&self) -> &serde_json::Value {
        match self {
            Passport::Human { claims, .. } | Passport::Service { claims, .. } => claims,
        }
    }

    pub fn claim<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.claims()
            .get(key)
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }

    pub fn scopes(&self) -> Vec<ScopeKey> {
        self.claim::<Vec<String>>(SCOPES_CLAIM_KEY)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|raw| ScopeKey::new(raw).ok())
            .collect()
    }

    pub fn has_scope(&self, scope: &ScopeKey) -> bool {
        self.claim::<Vec<String>>(SCOPES_CLAIM_KEY)
            .unwrap_or_default()
            .iter()
            .any(|raw| raw == scope.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn human(admin: bool, active: bool) -> Passport {
        Passport::Human {
            user_id: Uuid::nil(),
            is_super_admin: admin,
            is_active: active,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: json!({"email": "alice@example.com", "role": "manager"}),
        }
    }

    fn pat_human() -> Passport {
        Passport::Human {
            user_id: Uuid::from_u128(1),
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Pat {
                token_id: Uuid::from_u128(100),
            },
            impersonator: None,
            claims: json!({}),
        }
    }

    fn impersonated_human() -> Passport {
        Passport::Human {
            user_id: Uuid::from_u128(1),
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: Some(Uuid::from_u128(999)),
            claims: json!({}),
        }
    }

    fn service() -> Passport {
        Passport::Service {
            service_account_id: Uuid::from_u128(42),
            claims: json!({"name": "ci-bot"}),
        }
    }

    #[test]
    fn actor_id_returns_user_id_for_human() {
        let uid = Uuid::from_u128(99);
        let p = Passport::Human {
            user_id: uid,
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: json!({}),
        };
        assert_eq!(p.actor_id(), uid);
    }

    #[test]
    fn actor_id_returns_service_account_id_for_service() {
        let sid = Uuid::from_u128(77);
        let p = Passport::Service {
            service_account_id: sid,
            claims: json!({}),
        };
        assert_eq!(p.actor_id(), sid);
    }

    #[test]
    fn actor_id_returns_impersonated_user_not_admin() {
        let p = impersonated_human();
        assert_eq!(p.actor_id(), Uuid::from_u128(1));
        assert_ne!(p.actor_id(), Uuid::from_u128(999));
    }

    #[test]
    fn to_actor_maps_human_to_human_user_id() {
        let uid = Uuid::from_u128(99);
        let p = Passport::Human {
            user_id: uid,
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: json!({}),
        };
        assert_eq!(p.to_actor(), Actor::Human(UserId::from(uid)));
    }

    #[test]
    fn to_actor_maps_service_to_service_account_id() {
        let sid = Uuid::from_u128(77);
        let p = Passport::Service {
            service_account_id: sid,
            claims: json!({}),
        };
        assert_eq!(p.to_actor(), Actor::Service(ServiceAccountId::from(sid)));
    }

    #[test]
    fn is_super_admin_true_for_admin_human() {
        assert!(human(true, true).is_super_admin());
    }

    #[test]
    fn is_super_admin_false_for_non_admin_human() {
        assert!(!human(false, true).is_super_admin());
    }

    #[test]
    fn is_super_admin_false_for_service() {
        assert!(!service().is_super_admin());
    }

    #[test]
    fn is_active_true_for_active_human() {
        assert!(human(false, true).is_active());
    }

    #[test]
    fn is_active_false_for_inactive_human() {
        assert!(!human(false, false).is_active());
    }

    #[test]
    fn is_active_always_true_for_service() {
        assert!(service().is_active());
    }

    #[test]
    fn auth_method_returns_jwt_for_jwt_human() {
        assert_eq!(human(false, true).auth_method(), Some(&AuthMethod::Jwt));
    }

    #[test]
    fn auth_method_returns_pat_for_pat_human() {
        let p = pat_human();
        assert!(matches!(p.auth_method(), Some(AuthMethod::Pat { .. })));
    }

    #[test]
    fn auth_method_none_for_service() {
        assert!(service().auth_method().is_none());
    }

    #[test]
    fn is_pat_true_for_pat_human() {
        assert!(pat_human().is_pat());
    }

    #[test]
    fn is_pat_false_for_jwt_human() {
        assert!(!human(false, true).is_pat());
    }

    #[test]
    fn is_pat_false_for_service() {
        assert!(!service().is_pat());
    }

    #[test]
    fn is_impersonating_true_when_impersonator_set() {
        assert!(impersonated_human().is_impersonating());
    }

    #[test]
    fn is_impersonating_false_for_direct_human() {
        assert!(!human(false, true).is_impersonating());
    }

    #[test]
    fn is_impersonating_false_for_service() {
        assert!(!service().is_impersonating());
    }

    #[test]
    fn impersonator_id_returns_admin_uuid() {
        assert_eq!(
            impersonated_human().impersonator_id(),
            Some(Uuid::from_u128(999))
        );
    }

    #[test]
    fn impersonator_id_none_for_direct_human() {
        assert!(human(false, true).impersonator_id().is_none());
    }

    #[test]
    fn impersonator_id_none_for_service() {
        assert!(service().impersonator_id().is_none());
    }

    #[test]
    fn claims_returns_human_claims() {
        let p = human(false, true);
        assert_eq!(p.claims()["email"], "alice@example.com");
    }

    #[test]
    fn claims_returns_service_claims() {
        let p = service();
        assert_eq!(p.claims()["name"], "ci-bot");
    }

    #[test]
    fn claim_extracts_typed_value() {
        let p = human(false, true);
        let email: Option<String> = p.claim("email");
        assert_eq!(email.as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn claim_returns_none_for_missing_key() {
        let p = human(false, true);
        let missing: Option<String> = p.claim("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn claim_returns_none_for_type_mismatch() {
        let p = human(false, true);
        let bad: Option<i32> = p.claim("email");
        assert!(bad.is_none());
    }

    #[test]
    fn serde_roundtrip_human() {
        let p = human(true, true);
        let json = serde_json::to_string(&p).unwrap();
        let back: Passport = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn serde_roundtrip_pat_human() {
        let p = pat_human();
        let json = serde_json::to_string(&p).unwrap();
        let back: Passport = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn serde_roundtrip_impersonated_human() {
        let p = impersonated_human();
        let json = serde_json::to_string(&p).unwrap();
        let back: Passport = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn serde_roundtrip_service() {
        let p = service();
        let json = serde_json::to_string(&p).unwrap();
        let back: Passport = serde_json::from_str(&json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn json_discriminant_human() {
        let p = human(false, true);
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert_eq!(v["kind"], "human");
        assert!(v.get("user_id").is_some());
        assert!(v.get("is_super_admin").is_some());
        assert!(v.get("is_active").is_some());
        assert!(v.get("auth_method").is_some());
        assert!(v.get("claims").is_some());
    }

    #[test]
    fn json_human_auth_method_jwt_shape() {
        let p = human(false, true);
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert_eq!(v["auth_method"]["method"], "jwt");
    }

    #[test]
    fn json_human_auth_method_pat_shape() {
        let p = pat_human();
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert_eq!(v["auth_method"]["method"], "pat");
        assert_eq!(
            v["auth_method"]["token_id"],
            Uuid::from_u128(100).to_string()
        );
    }

    #[test]
    fn json_discriminant_service() {
        let p = service();
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert_eq!(v["kind"], "service");
        assert!(v.get("service_account_id").is_some());
        assert!(v.get("claims").is_some());
        assert!(v.get("user_id").is_none());
        assert!(v.get("is_super_admin").is_none());
        assert!(v.get("auth_method").is_none());
        assert!(v.get("impersonator").is_none());
    }

    #[test]
    fn deserialize_human_from_json() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"},
            "claims": {"email": "bob@example.com"}
        }"#;
        let p: Passport = serde_json::from_str(json).unwrap();
        assert!(matches!(p, Passport::Human { .. }));
        assert_eq!(
            p.claim::<String>("email").as_deref(),
            Some("bob@example.com")
        );
        assert!(!p.is_pat());
        assert!(!p.is_impersonating());
    }

    #[test]
    fn deserialize_human_pat_from_json() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000001",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "pat", "token_id": "00000000-0000-0000-0000-000000000064"},
            "claims": {}
        }"#;
        let p: Passport = serde_json::from_str(json).unwrap();
        assert!(p.is_pat());
    }

    #[test]
    fn deserialize_human_impersonated_from_json() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000001",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"},
            "impersonator": "00000000-0000-0000-0000-0000000003e7",
            "claims": {}
        }"#;
        let p: Passport = serde_json::from_str(json).unwrap();
        assert!(p.is_impersonating());
        assert_eq!(p.impersonator_id(), Some(Uuid::from_u128(999)));
    }

    #[test]
    fn deserialize_human_accepts_missing_impersonator() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"},
            "claims": {}
        }"#;
        let p: Passport = serde_json::from_str(json).unwrap();
        assert!(!p.is_impersonating());
    }

    #[test]
    fn deserialize_human_rejects_missing_auth_method() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "claims": {}
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_service_from_json() {
        let json = r#"{
            "kind": "service",
            "service_account_id": "00000000-0000-0000-0000-00000000002a",
            "claims": {}
        }"#;
        let p: Passport = serde_json::from_str(json).unwrap();
        assert!(matches!(p, Passport::Service { .. }));
    }

    #[test]
    fn deserialize_rejects_missing_kind() {
        let json = r#"{"user_id":"00000000-0000-0000-0000-000000000000","is_super_admin":false,"is_active":true,"auth_method":{"method":"jwt"},"claims":{}}"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_kind() {
        let json = r#"{"kind":"robot","id":"00000000-0000-0000-0000-000000000000","claims":{}}"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_top_level_field_on_human() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"},
            "claims": {},
            "evil": true
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_top_level_field_on_service() {
        let json = r#"{
            "kind": "service",
            "service_account_id": "00000000-0000-0000-0000-00000000002a",
            "claims": {},
            "evil": true
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_absent_claims() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"}
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_absent_claims_on_service() {
        let json = r#"{
            "kind": "service",
            "service_account_id": "00000000-0000-0000-0000-00000000002a"
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_null_claims() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"},
            "claims": null
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_non_object_claims() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"},
            "claims": 42
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_non_object_claims_on_service() {
        let json = r#"{
            "kind": "service",
            "service_account_id": "00000000-0000-0000-0000-00000000002a",
            "claims": []
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_field_in_auth_method() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt", "evil": true},
            "claims": {}
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_duplicate_field() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "user_id": "00000000-0000-0000-0000-000000000001",
            "is_super_admin": false,
            "is_active": true,
            "auth_method": {"method": "jwt"},
            "claims": {}
        }"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn human_passports_with_same_fields_are_equal() {
        assert_eq!(human(true, true), human(true, true));
    }

    #[test]
    fn human_and_service_are_not_equal() {
        assert_ne!(human(false, true), service());
    }

    #[test]
    fn jwt_and_pat_humans_are_not_equal() {
        assert_ne!(human(false, true), pat_human());
    }

    #[test]
    fn impersonated_and_direct_humans_are_not_equal() {
        assert_ne!(human(false, true), impersonated_human());
    }

    fn human_with_scopes(scopes: serde_json::Value) -> Passport {
        Passport::Human {
            user_id: Uuid::from_u128(1),
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: json!({ "scopes": scopes }),
        }
    }

    #[test]
    fn scopes_parses_claim_strings_into_typed_keys() {
        let p = human_with_scopes(json!(["notifier:read", "notifier:write"]));
        assert_eq!(
            p.scopes(),
            vec![
                ScopeKey::new("notifier:read").unwrap(),
                ScopeKey::new("notifier:write").unwrap(),
            ]
        );
    }

    #[test]
    fn scopes_empty_when_claim_absent() {
        assert!(human(false, true).scopes().is_empty());
    }

    #[test]
    fn scopes_empty_for_service_without_claim() {
        assert!(service().scopes().is_empty());
    }

    #[test]
    fn scopes_skips_malformed_entries_keeping_valid_ones() {
        let p = human_with_scopes(json!(["notifier:read", "Bad Scope", "a:b:c", ":empty"]));
        assert_eq!(p.scopes(), vec![ScopeKey::new("notifier:read").unwrap()]);
    }

    #[test]
    fn scopes_empty_when_claim_is_not_a_string_array() {
        assert!(
            human_with_scopes(json!("notifier:read"))
                .scopes()
                .is_empty()
        );
        assert!(human_with_scopes(json!([1, 2, 3])).scopes().is_empty());
    }

    #[test]
    fn has_scope_true_when_granted() {
        let p = human_with_scopes(json!(["notifier:read", "billing:manage"]));
        assert!(p.has_scope(&ScopeKey::new("billing:manage").unwrap()));
    }

    #[test]
    fn has_scope_false_when_not_granted() {
        let p = human_with_scopes(json!(["notifier:read"]));
        assert!(!p.has_scope(&ScopeKey::new("notifier:write").unwrap()));
    }

    #[test]
    fn has_scope_false_when_claim_absent() {
        assert!(!human(false, true).has_scope(&ScopeKey::new("notifier:read").unwrap()));
    }

    #[test]
    fn has_scope_unaffected_by_malformed_neighbours() {
        let p = human_with_scopes(json!(["Bad Scope", "notifier:read"]));
        assert!(p.has_scope(&ScopeKey::new("notifier:read").unwrap()));
    }

    #[test]
    fn empty_claims_are_valid() {
        let p = Passport::Human {
            user_id: Uuid::nil(),
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: json!({}),
        };
        assert_eq!(p.claims(), &json!({}));
        let nothing: Option<String> = p.claim("anything");
        assert!(nothing.is_none());
    }
}
