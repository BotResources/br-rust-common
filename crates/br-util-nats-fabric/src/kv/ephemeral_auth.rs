use std::collections::BTreeMap;
use std::marker::PhantomData;
use std::time::Duration;

use async_nats::jetstream::kv::Store;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::encode;
use crate::kv::ephemeral_auth_watch::EphemeralAuthWatcher;
use crate::kv::key::{KvKey, KvPrefix};
use crate::kv::revision::{
    Revision, create_absent, delete_expecting, map_create_error, read_with_revision,
    update_expecting,
};
use crate::kv::scan::{scan_entries, scan_keys};

pub struct EphemeralAuthStore<V> {
    kv: Store,
    _value: PhantomData<V>,
}

impl<V> EphemeralAuthStore<V> {
    pub(crate) fn store(&self) -> &Store {
        &self.kv
    }
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
        read_with_revision(&self.kv, key).await
    }

    pub async fn create(&self, key: &KvKey, value: &V) -> Result<(), FabricError> {
        create_absent(&self.kv, key, value).await?;
        Ok(())
    }

    pub async fn update_if(
        &self,
        key: &KvKey,
        value: &V,
        expected: Revision,
    ) -> Result<(), FabricError> {
        update_expecting(&self.kv, key, value, expected).await?;
        Ok(())
    }

    pub async fn delete(&self, key: &KvKey) -> Result<(), FabricError> {
        self.kv
            .delete(key.as_str())
            .await
            .map_err(FabricError::kv)?;
        Ok(())
    }

    pub async fn delete_if(&self, key: &KvKey, expected: Revision) -> Result<(), FabricError> {
        delete_expecting(&self.kv, key, expected).await
    }

    pub async fn put(&self, key: &KvKey, value: &V) -> Result<(), FabricError> {
        let bytes = encode(value)?;
        self.kv
            .put(key.as_str(), bytes.into())
            .await
            .map_err(FabricError::kv)?;
        Ok(())
    }

    pub async fn create_with_ttl(
        &self,
        key: &KvKey,
        value: &V,
        ttl: Duration,
    ) -> Result<(), FabricError> {
        let bytes = encode(value)?;
        match self
            .kv
            .create_with_ttl(key.as_str(), bytes.into(), ttl)
            .await
        {
            Ok(_) => Ok(()),
            Err(err) => Err(map_create_error(key, err.kind(), &err)),
        }
    }

    pub async fn keys(&self, prefix: &KvPrefix) -> Result<Vec<KvKey>, FabricError> {
        scan_keys(&self.kv, prefix).await
    }

    pub async fn entries(&self, prefix: &KvPrefix) -> Result<BTreeMap<KvKey, V>, FabricError> {
        scan_entries(&self.kv, prefix).await
    }

    pub fn watcher(&self) -> EphemeralAuthWatcher<V> {
        EphemeralAuthWatcher::bind(self)
    }
}
