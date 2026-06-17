use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;

use async_nats::jetstream::kv::{Operation, Store};
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::error::FabricError;
use crate::fabric::Fabric;
use crate::kv::codec::decode;
use crate::kv::health::{WatchHealth, WatchHealthChannel, WatchHealthReceiver};
use crate::kv::key::{KvKey, KvPrefix};
use crate::kv::sink::{ProjectionError, ProjectionSink};

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
        let subjects: Vec<String> = self.prefixes.iter().map(KvPrefix::watch_subject).collect();
        let mut entries = self
            .kv
            .watch_many(subjects)
            .await
            .map_err(|e| ProjectionError::Fabric(crate::error::FabricError::kv(e)))?;

        while let Some(entry) = entries.next().await {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    self.health.set(WatchHealth::Degraded);
                    return Err(ProjectionError::Fabric(crate::error::FabricError::kv(e)));
                }
            };
            self.health.set(WatchHealth::Healthy);
            self.apply_entry(entry).await?;
        }
        self.health.set(WatchHealth::Degraded);
        Ok(())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryAction {
    Project,
    Retract,
}

fn decide_put<V, F: Fn(&V) -> bool>(copy_filter: &F, value: &V) -> EntryAction {
    if copy_filter(value) {
        EntryAction::Project
    } else {
        EntryAction::Retract
    }
}

fn orphans<'a>(
    observed: &BTreeSet<KvKey>,
    desired: impl IntoIterator<Item = &'a KvKey>,
    prefixes: &[KvPrefix],
) -> Vec<KvKey> {
    let desired: BTreeSet<&KvKey> = desired.into_iter().collect();
    observed
        .iter()
        .filter(|key| prefixes.iter().any(|p| p.matches(key.as_str())))
        .filter(|key| !desired.contains(*key))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> KvKey {
        KvKey::new(s).unwrap()
    }

    fn prefix(s: &str) -> KvPrefix {
        KvPrefix::new(s).unwrap()
    }

    #[test]
    fn orphans_are_observed_keys_under_a_watched_prefix_absent_from_desired() {
        let observed = BTreeSet::from([
            key("identity/users/1"),
            key("identity/users/2"),
            key("identity/users/3"),
        ]);
        let desired = [key("identity/users/2"), key("identity/users/3")];
        let prefixes = [prefix("identity/users/")];
        assert_eq!(
            orphans(&observed, desired.iter(), &prefixes),
            vec![key("identity/users/1")]
        );
    }

    #[test]
    fn orphan_detection_ignores_keys_outside_the_selected_prefixes() {
        let observed = BTreeSet::from([key("identity/groups/9")]);
        let desired: [KvKey; 0] = [];
        let prefixes = [prefix("identity/users/")];
        assert!(orphans(&observed, desired.iter(), &prefixes).is_empty());
    }

    #[derive(PartialEq, Eq)]
    struct Membership {
        active: bool,
    }

    #[test]
    fn a_passing_value_is_projected() {
        let filter = |m: &Membership| m.active;
        assert_eq!(
            decide_put(&filter, &Membership { active: true }),
            EntryAction::Project
        );
    }

    #[test]
    fn a_value_that_flips_pass_to_fail_is_retracted_locally() {
        let filter = |m: &Membership| m.active;
        assert_eq!(
            decide_put(&filter, &Membership { active: false }),
            EntryAction::Retract
        );
    }

    #[test]
    fn empty_desired_orphans_every_observed_key_under_prefix() {
        let observed = BTreeSet::from([key("identity/users/1"), key("identity/users/2")]);
        let desired: [KvKey; 0] = [];
        let prefixes = [prefix("identity/users/")];
        assert_eq!(
            orphans(&observed, desired.iter(), &prefixes),
            vec![key("identity/users/1"), key("identity/users/2")]
        );
    }
}
