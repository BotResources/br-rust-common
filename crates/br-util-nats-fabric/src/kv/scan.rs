use std::collections::BTreeMap;

use async_nats::jetstream::kv::Store;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::kv::codec::decode;
use crate::kv::key::{KvKey, KvPrefix};

pub(crate) async fn scan_entries<V: DeserializeOwned>(
    kv: &Store,
    prefix: &KvPrefix,
) -> Result<BTreeMap<KvKey, V>, FabricError> {
    let mut keys = kv.keys().await.map_err(FabricError::kv)?;
    let mut entries = BTreeMap::new();
    while let Some(key) = keys.next().await {
        let key = key.map_err(FabricError::kv)?;
        if !prefix.matches(&key) {
            continue;
        }
        let Some(bytes) = kv.get(&key).await.map_err(FabricError::kv)? else {
            continue;
        };
        let value = decode(&key, &bytes)?;
        entries.insert(KvKey::new(key)?, value);
    }
    Ok(entries)
}

pub(crate) async fn scan_keys(kv: &Store, prefix: &KvPrefix) -> Result<Vec<KvKey>, FabricError> {
    let mut keys = kv.keys().await.map_err(FabricError::kv)?;
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
