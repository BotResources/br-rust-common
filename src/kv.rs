use async_trait::async_trait;
use thiserror::Error;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

#[derive(Debug, Error)]
pub enum KvError {
    #[error("key not found: {0}")]
    NotFound(String),

    #[error("storage error: {0}")]
    Storage(String),

    #[error("configuration error: {0}")]
    Config(String),
}

// ---------------------------------------------------------------------------
// Port trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait KvPorts: Send + Sync {
    /// Create or update a key-value pair (upsert semantics).
    async fn put(&self, key: &str, value: serde_json::Value) -> Result<(), KvError>;

    /// Get a value by key. Returns `None` if the key does not exist.
    async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, KvError>;

    /// Delete a key. Idempotent — deleting a non-existent key succeeds.
    async fn delete(&self, key: &str) -> Result<(), KvError>;

    /// List all key-value pairs where the key starts with `prefix`.
    async fn list_by_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<(String, serde_json::Value)>, KvError>;
}

// ---------------------------------------------------------------------------
// In-memory implementation (test / test-support)
// ---------------------------------------------------------------------------

#[cfg(any(test, feature = "test-support"))]
pub mod in_memory {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Debug, Default)]
    pub struct InMemoryKv {
        store: Mutex<HashMap<String, serde_json::Value>>,
    }

    impl InMemoryKv {
        pub fn new() -> Self {
            Self::default()
        }
    }

    #[async_trait]
    impl KvPorts for InMemoryKv {
        async fn put(&self, key: &str, value: serde_json::Value) -> Result<(), KvError> {
            self.store
                .lock()
                .map_err(|e| KvError::Storage(e.to_string()))?
                .insert(key.to_string(), value);
            Ok(())
        }

        async fn get(&self, key: &str) -> Result<Option<serde_json::Value>, KvError> {
            Ok(self
                .store
                .lock()
                .map_err(|e| KvError::Storage(e.to_string()))?
                .get(key)
                .cloned())
        }

        async fn delete(&self, key: &str) -> Result<(), KvError> {
            self.store
                .lock()
                .map_err(|e| KvError::Storage(e.to_string()))?
                .remove(key);
            Ok(())
        }

        async fn list_by_prefix(
            &self,
            prefix: &str,
        ) -> Result<Vec<(String, serde_json::Value)>, KvError> {
            let guard = self
                .store
                .lock()
                .map_err(|e| KvError::Storage(e.to_string()))?;
            let results: Vec<_> = guard
                .iter()
                .filter(|(k, _)| k.starts_with(prefix))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            Ok(results)
        }
    }
}

#[cfg(any(test, feature = "test-support"))]
pub use in_memory::InMemoryKv;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_and_get() {
        let kv = InMemoryKv::new();
        kv.put("key1", serde_json::json!({"name": "test"}))
            .await
            .unwrap();
        let val = kv.get("key1").await.unwrap();
        assert_eq!(val, Some(serde_json::json!({"name": "test"})));
    }

    #[tokio::test]
    async fn get_missing_returns_none() {
        let kv = InMemoryKv::new();
        assert_eq!(kv.get("missing").await.unwrap(), None);
    }

    #[tokio::test]
    async fn put_overwrites() {
        let kv = InMemoryKv::new();
        kv.put("k", serde_json::json!(1)).await.unwrap();
        kv.put("k", serde_json::json!(2)).await.unwrap();
        assert_eq!(kv.get("k").await.unwrap(), Some(serde_json::json!(2)));
    }

    #[tokio::test]
    async fn delete_is_idempotent() {
        let kv = InMemoryKv::new();
        kv.put("k", serde_json::json!(1)).await.unwrap();
        kv.delete("k").await.unwrap();
        assert_eq!(kv.get("k").await.unwrap(), None);
        // Deleting again should succeed
        kv.delete("k").await.unwrap();
    }

    #[tokio::test]
    async fn list_by_prefix_filters() {
        let kv = InMemoryKv::new();
        kv.put("identity/users/1", serde_json::json!({"email": "a"}))
            .await
            .unwrap();
        kv.put("identity/users/2", serde_json::json!({"email": "b"}))
            .await
            .unwrap();
        kv.put("identity/orgs/1", serde_json::json!({"name": "org"}))
            .await
            .unwrap();

        let users = kv.list_by_prefix("identity/users/").await.unwrap();
        assert_eq!(users.len(), 2);

        let orgs = kv.list_by_prefix("identity/orgs/").await.unwrap();
        assert_eq!(orgs.len(), 1);

        let all = kv.list_by_prefix("identity/").await.unwrap();
        assert_eq!(all.len(), 3);

        let none = kv.list_by_prefix("nonexistent/").await.unwrap();
        assert!(none.is_empty());
    }
}
