use std::collections::{BTreeMap, BTreeSet};

use async_nats::jetstream::kv::Store;
use br_core_directory::{
    DirectoryMeta, META_KEY, PublishedGroup, PublishedUser, group_id_from_kv_key,
    user_id_from_kv_key,
};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;
use sqlx::PgPool;
use uuid::Uuid;

use crate::consumer::recompose::member_rows;
use crate::error::DirectoryError;

pub struct DirectoryProjector {
    kv: Store,
    pool: PgPool,
}

impl DirectoryProjector {
    pub fn new(kv: Store, pool: PgPool) -> Self {
        Self { kv, pool }
    }

    pub async fn reconcile(&self) -> Result<DirectoryMeta, DirectoryError> {
        let manifest = self.read_manifest().await?;

        let desired_users = self
            .kv_entries::<PublishedUser>(user_id_from_kv_key)
            .await?;
        for (user_id, user) in &desired_users {
            self.upsert_user(*user_id, user).await?;
        }
        for user_id in orphans(self.known_user_ids().await?, desired_users.keys()) {
            self.delete_user(user_id).await?;
        }

        let desired_groups = if manifest.publishes_groups() {
            self.kv_entries::<PublishedGroup>(group_id_from_kv_key)
                .await?
        } else {
            BTreeMap::new()
        };
        for (group_id, group) in &desired_groups {
            self.upsert_group(*group_id, group).await?;
        }
        for group_id in orphans(self.known_group_ids().await?, desired_groups.keys()) {
            self.delete_group(group_id).await?;
        }

        Ok(manifest)
    }

    pub async fn apply_user(
        &self,
        user_id: Uuid,
        user: &PublishedUser,
    ) -> Result<(), DirectoryError> {
        self.upsert_user(user_id, user).await
    }

    pub async fn remove_user(&self, user_id: Uuid) -> Result<(), DirectoryError> {
        self.delete_user(user_id).await
    }

    pub async fn apply_group(
        &self,
        group_id: Uuid,
        group: &PublishedGroup,
    ) -> Result<(), DirectoryError> {
        self.upsert_group(group_id, group).await
    }

    pub async fn remove_group(&self, group_id: Uuid) -> Result<(), DirectoryError> {
        self.delete_group(group_id).await
    }

    async fn read_manifest(&self) -> Result<DirectoryMeta, DirectoryError> {
        match self
            .kv
            .get(META_KEY)
            .await
            .map_err(|e| DirectoryError::Kv(e.to_string()))?
        {
            Some(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| DirectoryError::Wire(e.to_string()))
            }
            None => {
                tracing::warn!(
                    key = META_KEY,
                    "directory manifest absent; treating roster as empty"
                );
                Ok(DirectoryMeta {
                    version: br_core_directory::DIRECTORY_META_VERSION,
                    entities: Vec::new(),
                })
            }
        }
    }

    async fn kv_entries<T: DeserializeOwned>(
        &self,
        id_from_key: fn(&str) -> Option<Uuid>,
    ) -> Result<BTreeMap<Uuid, T>, DirectoryError> {
        let mut keys = self
            .kv
            .keys()
            .await
            .map_err(|e| DirectoryError::Kv(e.to_string()))?;

        let mut entries = BTreeMap::new();
        while let Some(key) = keys.next().await {
            let key = key.map_err(|e| DirectoryError::Kv(e.to_string()))?;
            let Some(id) = id_from_key(&key) else {
                continue;
            };
            let Some(bytes) = self
                .kv
                .get(&key)
                .await
                .map_err(|e| DirectoryError::Kv(e.to_string()))?
            else {
                continue;
            };
            let value =
                serde_json::from_slice(&bytes).map_err(|e| DirectoryError::Wire(e.to_string()))?;
            entries.insert(id, value);
        }
        Ok(entries)
    }

    async fn known_user_ids(&self) -> Result<BTreeSet<Uuid>, DirectoryError> {
        let ids: Vec<(Uuid,)> = sqlx::query_as("SELECT user_id FROM known_users")
            .fetch_all(&self.pool)
            .await?;
        Ok(ids.into_iter().map(|(id,)| id).collect())
    }

    async fn known_group_ids(&self) -> Result<BTreeSet<Uuid>, DirectoryError> {
        let ids: Vec<(Uuid,)> = sqlx::query_as("SELECT group_id FROM known_groups")
            .fetch_all(&self.pool)
            .await?;
        Ok(ids.into_iter().map(|(id,)| id).collect())
    }

    async fn upsert_user(&self, user_id: Uuid, user: &PublishedUser) -> Result<(), DirectoryError> {
        sqlx::query(
            "INSERT INTO known_users (user_id, email, first_name, last_name) \
             VALUES ($1, $2, $3, $4) \
             ON CONFLICT (user_id) DO UPDATE \
             SET email = EXCLUDED.email, \
                 first_name = EXCLUDED.first_name, \
                 last_name = EXCLUDED.last_name",
        )
        .bind(user_id)
        .bind(&user.email)
        .bind(&user.first_name)
        .bind(&user.last_name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn delete_user(&self, user_id: Uuid) -> Result<(), DirectoryError> {
        sqlx::query("DELETE FROM known_users WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn upsert_group(
        &self,
        group_id: Uuid,
        group: &PublishedGroup,
    ) -> Result<(), DirectoryError> {
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO known_groups (group_id, name) VALUES ($1, $2) \
             ON CONFLICT (group_id) DO UPDATE SET name = EXCLUDED.name",
        )
        .bind(group_id)
        .bind(&group.name)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM known_user_group WHERE group_id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;

        for row in member_rows(group_id, group) {
            sqlx::query(
                "INSERT INTO known_user_group (group_id, user_id) VALUES ($1, $2) \
                 ON CONFLICT (group_id, user_id) DO NOTHING",
            )
            .bind(row.group_id)
            .bind(row.user_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn delete_group(&self, group_id: Uuid) -> Result<(), DirectoryError> {
        sqlx::query("DELETE FROM known_groups WHERE group_id = $1")
            .bind(group_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

fn orphans<'a>(observed: BTreeSet<Uuid>, desired: impl IntoIterator<Item = &'a Uuid>) -> Vec<Uuid> {
    let desired: BTreeSet<Uuid> = desired.into_iter().copied().collect();
    observed.difference(&desired).copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    #[test]
    fn orphans_are_observed_ids_absent_from_desired() {
        let observed = BTreeSet::from([id(1), id(2), id(3)]);
        let desired = [id(2), id(3), id(4)];
        assert_eq!(orphans(observed, desired.iter()), vec![id(1)]);
    }

    #[test]
    fn no_orphans_when_observed_is_a_subset_of_desired() {
        let observed = BTreeSet::from([id(2)]);
        let desired = [id(1), id(2)];
        assert!(orphans(observed, desired.iter()).is_empty());
    }

    #[test]
    fn empty_desired_orphans_every_observed_id() {
        let observed = BTreeSet::from([id(1), id(2)]);
        assert_eq!(orphans(observed, [].iter()), vec![id(1), id(2)]);
    }
}
