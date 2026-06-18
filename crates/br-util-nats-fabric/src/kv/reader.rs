use std::collections::BTreeMap;
use std::marker::PhantomData;

use async_nats::jetstream::kv::Store;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::decode;
use crate::kv::key::{KvKey, KvPrefix};

pub struct PublishedLanguageReader<V> {
    kv: Store,
    _value: PhantomData<V>,
}

impl<V> PublishedLanguageReader<V>
where
    V: DeserializeOwned,
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

    pub async fn get(&self, key: &KvKey) -> Result<Option<V>, FabricError> {
        let Some(bytes) = self.kv.get(key.as_str()).await.map_err(FabricError::kv)? else {
            return Ok(None);
        };
        let value = decode(key.as_str(), &bytes)?;
        Ok(Some(value))
    }

    pub async fn keys(&self, prefix: &KvPrefix) -> Result<Vec<KvKey>, FabricError> {
        let mut keys = self.kv.keys().await.map_err(FabricError::kv)?;
        let mut matched = Vec::new();
        while let Some(key) = keys.next().await {
            let key = key.map_err(FabricError::kv)?;
            if !prefix.matches(&key) {
                continue;
            }
            matched.push(KvKey::new(key)?);
        }
        matched.sort();
        Ok(matched)
    }

    pub async fn entries(&self, prefix: &KvPrefix) -> Result<BTreeMap<KvKey, V>, FabricError> {
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
            entries.insert(KvKey::new(key)?, value);
        }
        Ok(entries)
    }
}
