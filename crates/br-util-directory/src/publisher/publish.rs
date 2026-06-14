use std::collections::BTreeMap;

use async_nats::jetstream::kv::Store;
use br_core_directory::{
    DirectoryMeta, META_KEY, PublishedGroup, PublishedUser, group_id_from_kv_key, group_kv_key,
    user_id_from_kv_key, user_kv_key,
};
use futures_util::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;
use uuid::Uuid;

use crate::error::DirectoryError;
use crate::publisher::reconcile::{KvOp, reconcile_entries};
use crate::publisher::source::DirectorySource;

pub struct DirectoryPublisher {
    kv: Store,
}

impl DirectoryPublisher {
    pub fn new(kv: Store) -> Self {
        Self { kv }
    }

    pub async fn reconcile<S: DirectorySource>(&self, source: &S) -> Result<(), DirectoryError> {
        let manifest = source.manifest();

        let desired_users = if manifest.publishes_users() {
            source.desired_users().await?
        } else {
            BTreeMap::new()
        };
        let observed_users = self.observed(user_id_from_kv_key).await?;
        self.apply(
            &reconcile_entries(&desired_users, &observed_users),
            user_kv_key,
        )
        .await?;

        let desired_groups = if manifest.publishes_groups() {
            source.desired_groups().await?
        } else {
            BTreeMap::new()
        };
        let observed_groups = self.observed(group_id_from_kv_key).await?;
        self.apply(
            &reconcile_entries(&desired_groups, &observed_groups),
            group_kv_key,
        )
        .await?;

        self.write_meta(&manifest).await
    }

    pub async fn publish_user(
        &self,
        user_id: Uuid,
        user: &PublishedUser,
    ) -> Result<(), DirectoryError> {
        self.put(&user_kv_key(user_id), user).await
    }

    pub async fn retract_user(&self, user_id: Uuid) -> Result<(), DirectoryError> {
        self.delete(&user_kv_key(user_id)).await
    }

    pub async fn publish_group(
        &self,
        group_id: Uuid,
        group: &PublishedGroup,
    ) -> Result<(), DirectoryError> {
        self.put(&group_kv_key(group_id), group).await
    }

    pub async fn retract_group(&self, group_id: Uuid) -> Result<(), DirectoryError> {
        self.delete(&group_kv_key(group_id)).await
    }

    pub async fn write_meta(&self, manifest: &DirectoryMeta) -> Result<(), DirectoryError> {
        self.put(META_KEY, manifest).await
    }

    async fn observed<T: DeserializeOwned>(
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

    async fn apply<T: Serialize>(
        &self,
        ops: &[KvOp<T>],
        key_for: fn(Uuid) -> String,
    ) -> Result<(), DirectoryError> {
        for op in ops {
            match op {
                KvOp::Put { id, value } => self.put(&key_for(*id), value).await?,
                KvOp::Delete { id } => self.delete(&key_for(*id)).await?,
            }
        }
        Ok(())
    }

    async fn put<T: Serialize>(&self, key: &str, value: &T) -> Result<(), DirectoryError> {
        let bytes = serde_json::to_vec(value).map_err(|e| DirectoryError::Wire(e.to_string()))?;
        self.kv
            .put(key, bytes.into())
            .await
            .map_err(|e| DirectoryError::Kv(e.to_string()))?;
        Ok(())
    }

    async fn delete(&self, key: &str) -> Result<(), DirectoryError> {
        self.kv
            .delete(key)
            .await
            .map_err(|e| DirectoryError::Kv(e.to_string()))?;
        Ok(())
    }
}
