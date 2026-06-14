use serde_json::{Map, Value};
use uuid::Uuid;

use crate::auth_method::AuthMethod;
use crate::passport::Passport;

pub struct PassportBuilder {
    user_id: Uuid,
    is_super_admin: bool,
    is_active: bool,
    auth_method: AuthMethod,
    impersonator: Option<Uuid>,
    claims: Map<String, Value>,
}

impl PassportBuilder {
    pub fn new() -> Self {
        Self {
            user_id: Uuid::now_v7(),
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: Map::new(),
        }
    }

    pub fn user_id(mut self, id: Uuid) -> Self {
        self.user_id = id;
        self
    }

    pub fn super_admin(mut self, is_super_admin: bool) -> Self {
        self.is_super_admin = is_super_admin;
        self
    }

    pub fn active(mut self, is_active: bool) -> Self {
        self.is_active = is_active;
        self
    }

    pub fn pat(mut self, token_id: Uuid) -> Self {
        self.auth_method = AuthMethod::Pat { token_id };
        self
    }

    pub fn impersonator(mut self, admin_id: Uuid) -> Self {
        self.impersonator = Some(admin_id);
        self
    }

    pub fn claim(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.claims.insert(key.into(), value.into());
        self
    }

    pub fn claims<K, V>(mut self, claims: impl IntoIterator<Item = (K, V)>) -> Self
    where
        K: Into<String>,
        V: Into<Value>,
    {
        self.claims
            .extend(claims.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    pub fn build(self) -> Passport {
        Passport::Human {
            user_id: self.user_id,
            is_super_admin: self.is_super_admin,
            is_active: self.is_active,
            auth_method: self.auth_method,
            impersonator: self.impersonator,
            claims: Value::Object(self.claims),
        }
    }

    pub fn build_service(self) -> Passport {
        Passport::Service {
            service_account_id: self.user_id,
            claims: Value::Object(self.claims),
        }
    }
}

impl Default for PassportBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::PassportHeader;

    #[test]
    fn human_defaults_are_active_non_super_admin_jwt() {
        let p = PassportBuilder::new().build();
        assert!(p.is_active());
        assert!(!p.is_super_admin());
        assert_eq!(p.auth_method(), Some(&AuthMethod::Jwt));
    }

    #[test]
    fn super_admin_sets_the_canonical_field() {
        let p = PassportBuilder::new().super_admin(true).build();
        assert!(p.is_super_admin());
    }

    #[test]
    fn active_false_forges_a_blocked_user() {
        let p = PassportBuilder::new().active(false).build();
        assert!(!p.is_active());
    }

    #[test]
    fn project_claims_land_in_the_claims_bag() {
        let p = PassportBuilder::new()
            .claim("email", "alice@example.com")
            .claim("scopes", vec!["a", "b"])
            .build();
        assert_eq!(
            p.claim::<String>("email").as_deref(),
            Some("alice@example.com")
        );
        assert_eq!(
            p.claim::<Vec<String>>("scopes"),
            Some(vec!["a".into(), "b".into()])
        );
    }

    #[test]
    fn claims_sets_several_at_once_and_repeats_overwrite() {
        let p = PassportBuilder::new()
            .claims([
                ("org_id", serde_json::json!("acme")),
                ("is_admin", serde_json::json!(true)),
            ])
            .claim("org_id", "globex")
            .build();
        assert_eq!(p.claim::<String>("org_id").as_deref(), Some("globex"));
        assert_eq!(p.claim::<bool>("is_admin"), Some(true));
    }

    #[test]
    fn pat_sets_auth_method() {
        let token_id = Uuid::now_v7();
        let p = PassportBuilder::new().pat(token_id).build();
        assert!(p.is_pat());
    }

    #[test]
    fn impersonation_keeps_effective_identity() {
        let admin = Uuid::now_v7();
        let user = Uuid::now_v7();
        let p = PassportBuilder::new()
            .user_id(user)
            .impersonator(admin)
            .build();
        assert_eq!(p.actor_id(), user);
        assert_eq!(p.impersonator_id(), Some(admin));
    }

    #[test]
    fn service_builds_with_project_claims() {
        let id = Uuid::now_v7();
        let p = PassportBuilder::new()
            .user_id(id)
            .claim("name", "ci-bot")
            .build_service();
        assert_eq!(p.actor_id(), id);
        assert_eq!(p.claim::<String>("name").as_deref(), Some("ci-bot"));
    }

    #[test]
    fn scope_claim_round_trips_through_the_typed_api() {
        let p = PassportBuilder::new()
            .claim("scopes", vec!["notifier:read", "notifier:write"])
            .build();
        assert!(p.has_scope(&crate::ScopeKey::new("notifier:read").unwrap()));
        assert_eq!(p.scopes().len(), 2);
    }

    #[test]
    fn roundtrips_through_the_x_passport_header() {
        let p = PassportBuilder::new()
            .claim("email", "bob@example.com")
            .claim("scopes", vec!["read"])
            .build();
        let header = p.to_header();
        let decoded = Passport::from_header(&header).expect("decode");
        assert_eq!(p, decoded);
    }
}
