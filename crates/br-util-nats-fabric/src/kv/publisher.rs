use std::collections::BTreeMap;
use std::marker::PhantomData;

use async_nats::jetstream::kv::Store;
use futures_util::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::{decode, encode};
use crate::kv::key::{KvKey, KvPrefix};
use crate::kv::reconcile::{KvOp, reconcile};

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
        let mut keys = self.kv.keys().await.map_err(FabricError::kv)?;
        let mut entries = BTreeMap::new();
        while let Some(key) = keys.next().await {
            let key = key.map_err(FabricError::kv)?;
            if !prefix.matches(&key) {
                continue;
            }
            let Some(bytes) = self.kv.get(&key).await.map_err(FabricError::kv)? else {
                continue;
            };
            let value = decode(&key, &bytes)?;
            let typed = KvKey::new(key)?;
            entries.insert(typed, value);
        }
        Ok(entries)
    }
}
