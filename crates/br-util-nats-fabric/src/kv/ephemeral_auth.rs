use std::marker::PhantomData;

use async_nats::jetstream::kv::{
    CreateErrorKind, DeleteErrorKind, Operation, Store, UpdateErrorKind,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::{decode, encode};
use crate::kv::key::KvKey;
use crate::kv::revision::Revision;

pub struct EphemeralAuthStore<V> {
    kv: Store,
    _value: PhantomData<V>,
}

impl<V> EphemeralAuthStore<V>
where
    V: Serialize + DeserializeOwned,
{
    pub async fn open(fabric: &Fabric) -> Result<Self, FabricError> {
        Ok(Self::bind(fabric.ephemeral_auth().await?))
    }

    pub(crate) fn bind(kv: Store) -> Self {
        Self {
            kv,
            _value: PhantomData,
        }
    }

    pub async fn status(&self) -> Result<(), FabricError> {
        self.kv.status().await.map_err(FabricError::kv)?;
        Ok(())
    }

    pub async fn get_with_revision(
        &self,
        key: &KvKey,
    ) -> Result<Option<(V, Revision)>, FabricError> {
        let Some(entry) = self.kv.entry(key.as_str()).await.map_err(FabricError::kv)? else {
            return Ok(None);
        };
        if matches!(entry.operation, Operation::Delete | Operation::Purge) {
            return Ok(None);
        }
        let value = decode(key.as_str(), &entry.value)?;
        Ok(Some((value, Revision::new(entry.revision))))
    }

    pub async fn create(&self, key: &KvKey, value: &V) -> Result<(), FabricError> {
        let bytes = encode(value)?;
        match self.kv.create(key.as_str(), bytes.into()).await {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == CreateErrorKind::AlreadyExists => {
                Err(FabricError::key_already_exists(key.as_str()))
            }
            Err(err) => Err(FabricError::kv(err)),
        }
    }

    pub async fn update_if(
        &self,
        key: &KvKey,
        value: &V,
        expected: Revision,
    ) -> Result<(), FabricError> {
        let bytes = encode(value)?;
        match self
            .kv
            .update(key.as_str(), bytes.into(), expected.get())
            .await
        {
            Ok(_) => Ok(()),
            Err(err) if err.kind() == UpdateErrorKind::WrongLastRevision => {
                Err(FabricError::revision_conflict(key.as_str(), expected.get()))
            }
            Err(err) => Err(FabricError::kv(err)),
        }
    }

    pub async fn delete(&self, key: &KvKey) -> Result<(), FabricError> {
        self.kv
            .delete(key.as_str())
            .await
            .map_err(FabricError::kv)?;
        Ok(())
    }

    pub async fn delete_if(&self, key: &KvKey, expected: Revision) -> Result<(), FabricError> {
        match self
            .kv
            .delete_expect_revision(key.as_str(), Some(expected.get()))
            .await
        {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == DeleteErrorKind::WrongLastRevision => {
                Err(FabricError::revision_conflict(key.as_str(), expected.get()))
            }
            Err(err) => Err(FabricError::kv(err)),
        }
    }

    pub async fn put(&self, key: &KvKey, value: &V) -> Result<(), FabricError> {
        let bytes = encode(value)?;
        self.kv
            .put(key.as_str(), bytes.into())
            .await
            .map_err(FabricError::kv)?;
        Ok(())
    }
}
