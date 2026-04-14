use serde::{Deserialize, Serialize, de::DeserializeOwned};
use uuid::Uuid;

/// The authenticated caller's identity, built by svc-identity and consumed
/// by all downstream services. Serialized to JSON, base64-encoded into the
/// `X-Passport` header.
///
/// Tagged enum with two variants: `Human` (person authenticated via JWT or PAT)
/// and `Service` (machine identity authenticated via API key).
///
/// Three typed fields on Human are universal (used by RLS in every project):
/// - `user_id` — identity
/// - `is_super_admin` — platform-level admin access
/// - `is_active` — whether the user is active or blocked
///
/// Everything else goes in `claims` — a free-form JSON bag that each project
/// fills with whatever it needs (email, role, tenant_id, etc.).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Passport {
    Human {
        user_id: Uuid,
        is_super_admin: bool,
        is_active: bool,
        claims: serde_json::Value,
    },
    Service {
        service_account_id: Uuid,
        claims: serde_json::Value,
    },
}

impl Passport {
    /// Returns the actor's UUID.
    ///
    /// - `Human` returns `user_id`
    /// - `Service` returns `service_account_id`
    pub fn actor_id(&self) -> Uuid {
        match self {
            Passport::Human { user_id, .. } => *user_id,
            Passport::Service {
                service_account_id, ..
            } => *service_account_id,
        }
    }

    /// Returns `true` only for `Human { is_super_admin: true, .. }`.
    /// Service accounts are never super admin.
    pub fn is_super_admin(&self) -> bool {
        matches!(self, Passport::Human { is_super_admin: true, .. })
    }

    /// Returns `true` for `Human { is_active: true, .. }`.
    /// Service accounts are always considered active.
    pub fn is_active(&self) -> bool {
        match self {
            Passport::Human { is_active, .. } => *is_active,
            Passport::Service { .. } => true,
        }
    }

    /// Returns a reference to the claims bag.
    pub fn claims(&self) -> &serde_json::Value {
        match self {
            Passport::Human { claims, .. } | Passport::Service { claims, .. } => claims,
        }
    }

    /// Extract a typed value from the claims bag by key.
    /// Returns `None` if the key is missing or deserialization fails.
    pub fn claim<T: DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.claims().get(key).and_then(|v| serde_json::from_value(v.clone()).ok())
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
            claims: json!({"email": "alice@example.com", "role": "manager"}),
        }
    }

    fn service() -> Passport {
        Passport::Service {
            service_account_id: Uuid::from_u128(42),
            claims: json!({"name": "ci-bot"}),
        }
    }

    // ─── actor_id ─────────────────────────────────────

    #[test]
    fn actor_id_returns_user_id_for_human() {
        let uid = Uuid::from_u128(99);
        let p = Passport::Human {
            user_id: uid,
            is_super_admin: false,
            is_active: true,
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

    // ─── is_super_admin ───────────────────────────────

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

    // ─── is_active ────────────────────────────────────

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

    // ─── claims ───────────────────────────────────────

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
        let bad: Option<i32> = p.claim("email"); // email is a string, not i32
        assert!(bad.is_none());
    }

    // ─── serde ────────────────────────────────────────

    #[test]
    fn serde_roundtrip_human() {
        let p = human(true, true);
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
        assert!(v.get("claims").is_some());
    }

    #[test]
    fn json_discriminant_service() {
        let p = service();
        let v: serde_json::Value = serde_json::to_value(&p).unwrap();
        assert_eq!(v["kind"], "service");
        assert!(v.get("service_account_id").is_some());
        assert!(v.get("claims").is_some());
        // Service should not have human fields
        assert!(v.get("user_id").is_none());
        assert!(v.get("is_super_admin").is_none());
    }

    #[test]
    fn deserialize_human_from_json() {
        let json = r#"{
            "kind": "human",
            "user_id": "00000000-0000-0000-0000-000000000000",
            "is_super_admin": false,
            "is_active": true,
            "claims": {"email": "bob@example.com"}
        }"#;
        let p: Passport = serde_json::from_str(json).unwrap();
        assert!(matches!(p, Passport::Human { .. }));
        assert_eq!(p.claim::<String>("email").as_deref(), Some("bob@example.com"));
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
        let json = r#"{"user_id":"00000000-0000-0000-0000-000000000000","is_super_admin":false,"is_active":true,"claims":{}}"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    #[test]
    fn deserialize_rejects_unknown_kind() {
        let json = r#"{"kind":"robot","id":"00000000-0000-0000-0000-000000000000","claims":{}}"#;
        assert!(serde_json::from_str::<Passport>(json).is_err());
    }

    // ─── equality ─────────────────────────────────────

    #[test]
    fn human_passports_with_same_fields_are_equal() {
        assert_eq!(human(true, true), human(true, true));
    }

    #[test]
    fn human_and_service_are_not_equal() {
        assert_ne!(human(false, true), service());
    }

    // ─── empty claims ─────────────────────────────────

    #[test]
    fn empty_claims_are_valid() {
        let p = Passport::Human {
            user_id: Uuid::nil(),
            is_super_admin: false,
            is_active: true,
            claims: json!({}),
        };
        assert_eq!(p.claims(), &json!({}));
        let nothing: Option<String> = p.claim("anything");
        assert!(nothing.is_none());
    }
}
