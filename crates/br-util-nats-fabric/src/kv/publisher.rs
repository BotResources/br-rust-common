use std::collections::BTreeMap;
use std::marker::PhantomData;

use async_nats::jetstream::kv::Store;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::encode;
use crate::kv::key::{KvKey, KvPrefix};
use crate::kv::reconcile::{KvOp, reconcile};
use crate::kv::revision::{Revision, delete_expecting, read_with_revision, update_expecting};
use crate::kv::scan::scan_entries;

pub struct PublishedLanguagePublisher<V> {
    kv: Store,
    _value: PhantomData<V>,
}

impl<V> PublishedLanguagePublisher<V>
where
    V: Serialize + DeserializeOwned + PartialEq + Clone,
{
    pub async fn open(fabric: &Fabric) -> Result<Self, FabricError> {
        Ok(Self::bind(fabric.published_language().await?))
    }

    pub(crate) fn bind(kv: Store) -> Self {
        Self {
            kv,
            _value: PhantomData,
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

    pub async fn update(&self, key: &KvKey, value: &V) -> Result<(), FabricError> {
        self.put(key, value).await
    }

    pub async fn get_with_revision(
        &self,
        key: &KvKey,
    ) -> Result<Option<(V, Revision)>, FabricError> {
        read_with_revision(&self.kv, key).await
    }

    pub async fn update_if(
        &self,
        key: &KvKey,
        value: &V,
        expected: Revision,
    ) -> Result<Revision, FabricError> {
        update_expecting(&self.kv, key, value, expected).await
    }

    pub async fn delete_if(&self, key: &KvKey, expected: Revision) -> Result<(), FabricError> {
        delete_expecting(&self.kv, key, expected).await
    }

    pub async fn retract(&self, key: &KvKey) -> Result<(), FabricError> {
        self.kv
            .delete(key.as_str())
            .await
            .map_err(FabricError::kv)?;
        Ok(())
    }

    pub async fn reconcile(
        &self,
        prefix: &KvPrefix,
        desired: &BTreeMap<KvKey, V>,
    ) -> Result<(), FabricError> {
        let observed = self.observed(prefix).await?;
        for op in reconcile(desired, &observed) {
            match op {
                KvOp::Put { key, value } => self.put(&key, &value).await?,
                KvOp::Delete { key } => self.retract(&key).await?,
            }
        }
        Ok(())
    }

    pub async fn repair_drift(
        &self,
        prefix: &KvPrefix,
        desired: &BTreeMap<KvKey, V>,
    ) -> Result<(), FabricError> {
        self.reconcile(prefix, desired).await
    }

    async fn observed(&self, prefix: &KvPrefix) -> Result<BTreeMap<KvKey, V>, FabricError> {
        scan_entries(&self.kv, prefix).await
    }
}
