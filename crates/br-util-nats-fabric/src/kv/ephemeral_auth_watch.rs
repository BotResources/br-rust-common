use std::marker::PhantomData;

use async_nats::jetstream::kv::{Operation, Store};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::consumer::backoff::Backoff;
use crate::error::FabricError;
use crate::kv::codec::decode;
use crate::kv::ephemeral_auth::EphemeralAuthStore;
use crate::kv::ephemeral_auth_supervise::SupervisedWatch;
use crate::kv::health::{WatchHealth, WatchHealthChannel, WatchHealthReceiver};
use crate::kv::key::KvKey;
use crate::kv::supervisor::supervise;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EphemeralAuthChange<V> {
    Set { key: KvKey, value: V },
    Removed { key: KvKey },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChangeKind {
    Set,
    Removed,
}

fn classify(operation: Operation) -> ChangeKind {
    match operation {
        Operation::Delete | Operation::Purge => ChangeKind::Removed,
        Operation::Put => ChangeKind::Set,
    }
}

pub struct EphemeralAuthWatcher<V> {
    kv: Store,
    health: WatchHealthChannel,
    _value: PhantomData<V>,
}

impl<V> EphemeralAuthWatcher<V>
where
    V: DeserializeOwned,
{
    pub(crate) fn bind(store: &EphemeralAuthStore<V>) -> Self {
        Self {
            kv: store.store().clone(),
            health: WatchHealthChannel::new(),
            _value: PhantomData,
        }
    }

    pub(crate) fn store(&self) -> &Store {
        &self.kv
    }

    pub fn health(&self) -> WatchHealthReceiver {
        self.health.receiver()
    }

    pub async fn run<H>(&self, on_change: H) -> std::convert::Infallible
    where
        V: DeserializeOwned + Send + Sync,
        H: FnMut(EphemeralAuthChange<V>) + Send,
    {
        supervise(
            &SupervisedWatch::new(self, on_change),
            &self.health,
            Backoff::production(),
            "ephemeral-auth",
        )
        .await
    }

    pub async fn watch<H>(&self, mut on_change: H) -> Result<(), FabricError>
    where
        V: DeserializeOwned + Send + Sync,
        H: FnMut(EphemeralAuthChange<V>) + Send,
    {
        let mut entries = self.kv.watch_all().await.map_err(FabricError::kv)?;
        while let Some(entry) = entries.next().await {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    self.health.set(WatchHealth::Degraded);
                    return Err(FabricError::kv(e));
                }
            };
            self.health.set(WatchHealth::Healthy);
            let change = match Self::change_from(entry) {
                Ok(change) => change,
                Err(e) => {
                    self.health.set(WatchHealth::Degraded);
                    return Err(e);
                }
            };
            on_change(change);
        }
        self.health.set(WatchHealth::Degraded);
        Ok(())
    }

    fn change_from(
        entry: async_nats::jetstream::kv::Entry,
    ) -> Result<EphemeralAuthChange<V>, FabricError> {
        let key = KvKey::new(entry.key.clone())?;
        Ok(match classify(entry.operation) {
            ChangeKind::Removed => EphemeralAuthChange::Removed { key },
            ChangeKind::Set => EphemeralAuthChange::Set {
                value: decode(&entry.key, &entry.value)?,
                key,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_classifies_as_set() {
        assert_eq!(classify(Operation::Put), ChangeKind::Set);
    }

    #[test]
    fn delete_classifies_as_removed() {
        assert_eq!(classify(Operation::Delete), ChangeKind::Removed);
    }

    #[test]
    fn purge_classifies_as_removed() {
        assert_eq!(classify(Operation::Purge), ChangeKind::Removed);
    }
}
