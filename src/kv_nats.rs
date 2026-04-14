use async_trait::async_trait;

use crate::kv::{KvError, KvPorts};

/// NATS JetStream KV-backed implementation.
///
/// Key translation: application keys use `/` as separator but NATS KV
/// does not allow `/` in keys — this adapter translates `/` → `.` transparently.
pub struct NatsKv {
    store: async_nats::jetstream::kv::Store,
}

impl NatsKv {
    pub fn new(store: async_nats::jetstream::kv::Store) -> Self {
        Self { store }
    }

    /// Translate application key (using `/`) to NATS key (using `.`).
    fn to_nats_key(key: &str) -> String {
        key.replace('/', ".")
    }

    /// Translate NATS key (using `.`) back to application key (using `/`).
    fn from_nats_key(key: &str) -> String {
        key.replace('.', "/")
    }
}

#[async_trait]
impl KvPorts for NatsKv {
    async fn put(&self, key: &str, value: serde_json::Value) -> Result<(), KvError> {
        let nats_key = Self::to_nats_key(key);
        let bytes = serde_json::to_vec(&value)
            .map_err(|e| KvError::Storage(format!("serialization error: {e}")))?;
        self.store
            .put(&nats_key, bytes.into())
            .await
            .map_err(|e| KvError::Storage(format!("NATS KV put error: {e}")))?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, KvError> {
        let nats_key = Self::to_nats_key(key);
        match self.store.get(&nats_key).await {
            Ok(Some(bytes)) => {
                let value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| KvError::Storage(format!("deserialization error: {e}")))?;
                Ok(Some(value))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(KvError::Storage(format!("NATS KV get error: {e}"))),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), KvError> {
        let nats_key = Self::to_nats_key(key);
        // Purge deletes all revisions of the key. Idempotent.
        self.store
            .purge(&nats_key)
            .await
            .map_err(|e| KvError::Storage(format!("NATS KV delete error: {e}")))?;
        Ok(())
    }

    async fn list_by_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, KvError> {
        let nats_prefix = Self::to_nats_key(prefix);
        let mut results = Vec::new();

        use futures::StreamExt;
        let mut keys = self
            .store
            .keys()
            .await
            .map_err(|e| KvError::Storage(format!("NATS KV keys error: {e}")))?;

        while let Some(key) = keys.next().await {
            let nats_key =
                key.map_err(|e| KvError::Storage(format!("NATS KV key iteration error: {e}")))?;
            if nats_key.starts_with(&nats_prefix)
                && let Ok(Some(bytes)) = self.store.get(&nats_key).await
                && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            {
                let app_key = Self::from_nats_key(&nats_key);
                results.push((app_key, value));
            }
        }

        Ok(results)
    }
}
