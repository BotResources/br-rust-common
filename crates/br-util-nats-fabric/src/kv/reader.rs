use std::collections::BTreeMap;
use std::marker::PhantomData;

use async_nats::jetstream::kv::Store;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::decode;
use crate::kv::key::{KvKey, KvPrefix};
use crate::kv::scan::{scan_entries, scan_keys};

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
        scan_keys(&self.kv, prefix).await
    }

    pub async fn entries(&self, prefix: &KvPrefix) -> Result<BTreeMap<KvKey, V>, FabricError> {
        scan_entries(&self.kv, prefix).await
    }
}
