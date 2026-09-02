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
use crate::kv::progress::{WatchProgressChannel, WatchProgressReceiver};
use crate::kv::supervisor::supervise;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EphemeralAuthChange<V> {
    Set { key: KvKey, value: V },
    Removed { key: KvKey },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SkipReason {
    InvalidKey,
    UndecodableValue,
}

impl SkipReason {
    fn as_str(self) -> &'static str {
        match self {
            Self::InvalidKey => "invalid key",
            Self::UndecodableValue => "undecodable value",
        }
    }
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
    progress: WatchProgressChannel,
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
            progress: WatchProgressChannel::new(),
            _value: PhantomData,
        }
    }

    pub(crate) fn store(&self) -> &Store {
        &self.kv
    }

    pub fn health(&self) -> WatchHealthReceiver {
        self.health.receiver()
    }

    pub fn progress(&self) -> WatchProgressReceiver {
        self.progress.receiver()
    }

    pub(crate) fn observed(&self) -> u64 {
        self.progress.snapshot().observed()
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
            let key = entry.key.clone();
            let value_len = entry.value.len();
            match Self::change_from(entry) {
                Ok(change) => {
                    on_change(change);
                    self.progress.record_change();
                }
                Err(reason) => {
                    self.progress.record_skip();
                    tracing::warn!(
                        surface = "ephemeral-auth",
                        key = %key,
                        reason = reason.as_str(),
                        value_len,
                        "skipping an ephemeral-auth entry this consumer cannot read"
                    );
                }
            }
        }
        self.health.set(WatchHealth::Degraded);
        Ok(())
    }

    fn change_from(
        entry: async_nats::jetstream::kv::Entry,
    ) -> Result<EphemeralAuthChange<V>, SkipReason> {
        let key = KvKey::new(entry.key.clone()).map_err(|_| SkipReason::InvalidKey)?;
        Ok(match classify(entry.operation) {
            ChangeKind::Removed => EphemeralAuthChange::Removed { key },
            ChangeKind::Set => EphemeralAuthChange::Set {
                value: decode::<V>(&entry.key, &entry.value)
                    .map_err(|_| SkipReason::UndecodableValue)?,
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

    #[test]
    fn skip_reasons_render_as_static_discriminants() {
        assert_eq!(SkipReason::InvalidKey.as_str(), "invalid key");
        assert_eq!(SkipReason::UndecodableValue.as_str(), "undecodable value");
    }
}
