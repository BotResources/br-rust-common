use std::collections::BTreeMap;
use std::marker::PhantomData;

use async_nats::jetstream::kv::{Operation, Store};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::consumer::backoff::Backoff;
use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::decode;
use crate::kv::health::{WatchHealthChannel, WatchHealthReceiver};
use crate::kv::key::{KvKey, KvPrefix};
use crate::kv::reconcile_ops::{EntryAction, decide_put, orphans};
use crate::kv::sink::{ProjectionError, ProjectionSink};
use crate::kv::supervisor::{FollowReport, ReconcileCycle, supervise};

pub struct PublishedLanguageConsumer<V, F, S> {
    kv: Store,
    prefixes: Vec<KvPrefix>,
    copy_filter: F,
    sink: S,
    health: WatchHealthChannel,
    _value: PhantomData<V>,
}

impl<V, F, S> PublishedLanguageConsumer<V, F, S>
where
    V: DeserializeOwned + Send + Sync,
    F: Fn(&V) -> bool,
    S: ProjectionSink<V>,
{
    pub async fn open(
        fabric: &Fabric,
        prefixes: Vec<KvPrefix>,
        copy_filter: F,
        sink: S,
    ) -> Result<Self, FabricError> {
        Ok(Self::bind(
            fabric.published_language().await?,
            prefixes,
            copy_filter,
            sink,
        ))
    }

    pub(crate) fn bind(kv: Store, prefixes: Vec<KvPrefix>, copy_filter: F, sink: S) -> Self {
        Self {
            kv,
            prefixes,
            copy_filter,
            sink,
            health: WatchHealthChannel::new(),
            _value: PhantomData,
        }
    }

    pub fn health(&self) -> WatchHealthReceiver {
        self.health.receiver()
    }

    pub async fn run(&self) -> std::convert::Infallible
    where
        F: Send + Sync,
        S::Error: std::fmt::Display,
    {
        supervise(self, &self.health, Backoff::production()).await
    }

    pub async fn bootstrap(&self) -> Result<(), ProjectionError<S::Error>> {
        let desired = self.scan_passing().await?;
        for (key, value) in &desired {
            self.sink
                .project(key, value)
                .await
                .map_err(ProjectionError::Sink)?;
        }
        let observed = self
            .sink
            .known_keys()
            .await
            .map_err(ProjectionError::Sink)?;
        for key in orphans(&observed, desired.keys(), &self.prefixes) {
            self.sink
                .retract(&key)
                .await
                .map_err(ProjectionError::Sink)?;
        }
        Ok(())
    }

    pub async fn watch(&self) -> Result<(), ProjectionError<S::Error>> {
        self.follow_once().await.1
    }

    async fn follow_once(&self) -> (bool, Result<(), ProjectionError<S::Error>>) {
        let mut entries = match self.kv.watch_all().await {
            Ok(entries) => entries,
            Err(e) => {
                return (
                    false,
                    Err(ProjectionError::Fabric(crate::error::FabricError::kv(e))),
                );
            }
        };

        let mut delivered = false;
        while let Some(entry) = entries.next().await {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    return (
                        delivered,
                        Err(ProjectionError::Fabric(crate::error::FabricError::kv(e))),
                    );
                }
            };
            delivered = true;
            if self.prefixes.iter().any(|p| p.matches(&entry.key))
                && let Err(e) = self.apply_entry(entry).await
            {
                return (delivered, Err(e));
            }
        }
        (delivered, Ok(()))
    }

    async fn apply_entry(
        &self,
        entry: async_nats::jetstream::kv::Entry,
    ) -> Result<(), ProjectionError<S::Error>> {
        let key = KvKey::new(entry.key.clone()).map_err(crate::error::FabricError::from)?;
        match entry.operation {
            Operation::Delete | Operation::Purge => {
                self.sink.retract(&key).await.map_err(ProjectionError::Sink)
            }
            Operation::Put => {
                let value: V = decode(&entry.key, &entry.value)?;
                match decide_put(&self.copy_filter, &value) {
                    EntryAction::Project => self
                        .sink
                        .project(&key, &value)
                        .await
                        .map_err(ProjectionError::Sink),
                    EntryAction::Retract => {
                        self.sink.retract(&key).await.map_err(ProjectionError::Sink)
                    }
                }
            }
        }
    }

    async fn scan_passing(&self) -> Result<BTreeMap<KvKey, V>, ProjectionError<S::Error>> {
        let mut keys = self
            .kv
            .keys()
            .await
            .map_err(|e| ProjectionError::Fabric(crate::error::FabricError::kv(e)))?;

        let mut passing = BTreeMap::new();
        while let Some(key) = keys.next().await {
            let key = key.map_err(|e| ProjectionError::Fabric(crate::error::FabricError::kv(e)))?;
            if !self.prefixes.iter().any(|p| p.matches(&key)) {
                continue;
            }
            let Some(bytes) = self
                .kv
                .get(&key)
                .await
                .map_err(|e| ProjectionError::Fabric(crate::error::FabricError::kv(e)))?
            else {
                continue;
            };
            let value: V = decode(&key, &bytes)?;
            if (self.copy_filter)(&value) {
                passing.insert(
                    KvKey::new(key).map_err(crate::error::FabricError::from)?,
                    value,
                );
            }
        }
        Ok(passing)
    }
}

#[async_trait::async_trait]
impl<V, F, S> ReconcileCycle for PublishedLanguageConsumer<V, F, S>
where
    V: DeserializeOwned + Send + Sync,
    F: Fn(&V) -> bool + Send + Sync,
    S: ProjectionSink<V>,
    S::Error: std::fmt::Display,
{
    async fn reconcile(&self) -> Result<(), String> {
        self.bootstrap().await.map_err(|e| e.to_string())
    }

    async fn follow(&self) -> FollowReport {
        let (progressed, outcome) = self.follow_once().await;
        FollowReport {
            progressed,
            outcome: outcome.map_err(|e| e.to_string()),
        }
    }
}
