use std::collections::BTreeSet;

use crate::kv::key::KvKey;

#[async_trait::async_trait]
pub trait ProjectionSink<V: Send + Sync>: Send + Sync {
    type Error;

    async fn project(&self, key: &KvKey, value: &V) -> Result<(), Self::Error>;

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error>;

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error>;
}

#[derive(thiserror::Error, Debug)]
pub enum ProjectionError<E> {
    #[error(transparent)]
    Fabric(#[from] crate::error::FabricError),
    #[error("projection sink failed: {0}")]
    Sink(E),
}
