use std::marker::PhantomData;

use async_nats::jetstream::kv::Store;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::decode;
use crate::kv::key::KvKey;

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
}
