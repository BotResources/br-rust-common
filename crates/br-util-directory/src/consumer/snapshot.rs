use std::collections::{BTreeMap, BTreeSet};

use br_core_directory::DirectoryMeta;
use serde_json::Value;
use uuid::Uuid;

use crate::consumer::config::PersistedExtensions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownUser {
    pub user_id: Uuid,
    pub email: String,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub extensions: PersistedExtensions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownServiceAccount {
    pub service_account_id: Uuid,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct DirectorySnapshot {
    groups_published: bool,
    service_accounts_published: bool,
    users: BTreeMap<Uuid, KnownUser>,
    group_names: BTreeMap<Uuid, String>,
    memberships: BTreeSet<(Uuid, Uuid)>,
    service_accounts: BTreeMap<Uuid, KnownServiceAccount>,
}

impl DirectorySnapshot {
    pub fn new(manifest: &DirectoryMeta) -> Self {
        Self {
            groups_published: manifest.publishes_groups(),
            service_accounts_published: manifest.publishes_service_accounts(),
            users: BTreeMap::new(),
            group_names: BTreeMap::new(),
            memberships: BTreeSet::new(),
            service_accounts: BTreeMap::new(),
        }
    }

    pub fn upsert_user(&mut self, user: KnownUser) {
        self.users.insert(user.user_id, user);
    }

    pub fn upsert_group(&mut self, group_id: Uuid, name: impl Into<String>) {
        self.group_names.insert(group_id, name.into());
    }

    pub fn set_members(&mut self, group_id: Uuid, member_ids: impl IntoIterator<Item = Uuid>) {
        self.memberships.retain(|(g, _)| *g != group_id);
        for user_id in member_ids {
            self.memberships.insert((group_id, user_id));
        }
    }

    pub fn upsert_service_account(&mut self, account: KnownServiceAccount) {
        self.service_accounts
            .insert(account.service_account_id, account);
    }

    pub fn resolve_user(&self, user_id: Uuid) -> Option<&KnownUser> {
        self.users.get(&user_id)
    }

    pub fn user_extensions(&self, user_id: Uuid) -> Option<&Value> {
        self.users
            .get(&user_id)
            .map(|user| user.extensions.as_value())
    }

    pub fn is_member(&self, group_id: Uuid, user_id: Uuid) -> bool {
        self.groups_published && self.memberships.contains(&(group_id, user_id))
    }

    pub fn group_name(&self, group_id: Uuid) -> Option<&str> {
        if !self.groups_published {
            return None;
        }
        self.group_names.get(&group_id).map(String::as_str)
    }

    pub fn resolve_service_account(
        &self,
        service_account_id: Uuid,
    ) -> Option<&KnownServiceAccount> {
        if !self.service_accounts_published {
            return None;
        }
        self.service_accounts.get(&service_account_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_directory::PublishedEntity;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn meta(entities: &[PublishedEntity]) -> DirectoryMeta {
        DirectoryMeta {
            version: 1,
            entities: entities.to_vec(),
        }
    }

    fn ada() -> KnownUser {
        KnownUser {
            user_id: id(1),
            email: "ada@example.com".to_string(),
            first_name: Some("Ada".to_string()),
            last_name: Some("Lovelace".to_string()),
            extensions: PersistedExtensions::from_value(serde_json::json!({ "locale": "en" })),
        }
    }

    #[test]
    fn resolve_user_returns_the_carried_id_and_fields() {
        let mut snap = DirectorySnapshot::new(&meta(&[PublishedEntity::Users]));
        snap.upsert_user(ada());
        let user = snap.resolve_user(id(1)).unwrap();
        assert_eq!(user.user_id, id(1));
        assert_eq!(user.email, "ada@example.com");
        assert_eq!(user.first_name.as_deref(), Some("Ada"));
        assert!(snap.resolve_user(id(99)).is_none());
    }

    #[test]
    fn extracted_extensions_survive_into_the_snapshot() {
        let mut snap = DirectorySnapshot::new(&meta(&[PublishedEntity::Users]));
        snap.upsert_user(ada());
        assert_eq!(
            snap.user_extensions(id(1)),
            Some(&serde_json::json!({ "locale": "en" }))
        );
        assert!(snap.user_extensions(id(99)).is_none());
    }

    #[test]
    fn group_readers_work_when_groups_published() {
        let mut snap =
            DirectorySnapshot::new(&meta(&[PublishedEntity::Users, PublishedEntity::Groups]));
        snap.upsert_group(id(10), "engineering");
        snap.set_members(id(10), [id(1), id(2)]);
        assert_eq!(snap.group_name(id(10)), Some("engineering"));
        assert!(snap.is_member(id(10), id(1)));
        assert!(!snap.is_member(id(10), id(3)));
    }

    #[test]
    fn group_readers_auto_degrade_when_groups_absent_from_manifest() {
        let mut snap = DirectorySnapshot::new(&meta(&[PublishedEntity::Users]));
        snap.upsert_group(id(10), "engineering");
        snap.set_members(id(10), [id(1)]);
        assert_eq!(snap.group_name(id(10)), None);
        assert!(!snap.is_member(id(10), id(1)));
        assert!(snap.resolve_user(id(1)).is_none());
    }

    #[test]
    fn membership_converges_when_the_group_projects_before_its_member_user() {
        let mut snap =
            DirectorySnapshot::new(&meta(&[PublishedEntity::Users, PublishedEntity::Groups]));
        snap.set_members(id(10), [id(1)]);
        assert!(snap.is_member(id(10), id(1)));
        assert!(snap.resolve_user(id(1)).is_none());

        snap.upsert_user(ada());
        assert!(snap.is_member(id(10), id(1)));
        assert!(snap.resolve_user(id(1)).is_some());
    }

    #[test]
    fn set_members_replaces_the_prior_membership_set_for_the_group() {
        let mut snap =
            DirectorySnapshot::new(&meta(&[PublishedEntity::Users, PublishedEntity::Groups]));
        snap.set_members(id(10), [id(1), id(2)]);
        snap.set_members(id(10), [id(3)]);
        assert!(!snap.is_member(id(10), id(1)));
        assert!(!snap.is_member(id(10), id(2)));
        assert!(snap.is_member(id(10), id(3)));
    }

    #[test]
    fn service_account_readers_work_when_declared() {
        let mut snap = DirectorySnapshot::new(&meta(&[
            PublishedEntity::Users,
            PublishedEntity::ServiceAccounts,
        ]));
        snap.upsert_service_account(KnownServiceAccount {
            service_account_id: id(20),
            name: "ci-runner".to_string(),
        });
        let account = snap.resolve_service_account(id(20)).unwrap();
        assert_eq!(account.name, "ci-runner");
    }

    #[test]
    fn service_account_readers_auto_degrade_when_absent_from_manifest() {
        let mut snap = DirectorySnapshot::new(&meta(&[PublishedEntity::Users]));
        snap.upsert_service_account(KnownServiceAccount {
            service_account_id: id(20),
            name: "ci-runner".to_string(),
        });
        assert!(snap.resolve_service_account(id(20)).is_none());
    }
}
