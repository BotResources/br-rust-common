use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::error::FabricError;
use crate::kv::ephemeral_auth_watch::{EphemeralAuthChange, EphemeralAuthWatcher};
use crate::kv::supervisor::{FollowReport, ReconcileCycle};

#[async_trait::async_trait]
pub(crate) trait ChangeSource {
    type Value;

    async fn bucket_present(&self) -> Result<(), FabricError>;

    async fn follow_changes(
        &self,
        on_change: &mut (dyn FnMut(EphemeralAuthChange<Self::Value>) + Send),
    ) -> Result<(), FabricError>;
}

#[async_trait::async_trait]
impl<V> ChangeSource for EphemeralAuthWatcher<V>
where
    V: DeserializeOwned + Send + Sync,
{
    type Value = V;

    async fn bucket_present(&self) -> Result<(), FabricError> {
        self.store()
            .stream
            .get_info()
            .await
            .map_err(FabricError::kv)?;
        Ok(())
    }

    async fn follow_changes(
        &self,
        on_change: &mut (dyn FnMut(EphemeralAuthChange<V>) + Send),
    ) -> Result<(), FabricError> {
        self.watch(on_change).await
    }
}

pub(crate) struct SupervisedWatch<'a, S: ?Sized, H> {
    source: &'a S,
    on_change: Mutex<H>,
}

impl<'a, S: ?Sized, H> SupervisedWatch<'a, S, H> {
    pub(crate) fn new(source: &'a S, on_change: H) -> Self {
        Self {
            source,
            on_change: Mutex::new(on_change),
        }
    }
}

#[async_trait::async_trait]
impl<S, H> ReconcileCycle for SupervisedWatch<'_, S, H>
where
    S: ChangeSource + Sync + ?Sized,
    S::Value: Send,
    H: FnMut(EphemeralAuthChange<S::Value>) + Send,
{
    async fn reconcile(&self) -> Result<(), String> {
        self.source
            .bucket_present()
            .await
            .map_err(|e| e.to_string())
    }

    async fn follow(&self) -> FollowReport {
        let mut handler = self.on_change.lock().await;
        let mut progressed = false;
        let outcome = {
            let mut record = |change| {
                progressed = true;
                (*handler)(change);
            };
            self.source.follow_changes(&mut record).await
        };
        FollowReport {
            progressed,
            outcome: outcome.map_err(|e| e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex as SyncMutex};
    use std::time::Duration;
    use tokio::sync::Notify;

    use crate::consumer::backoff::Backoff;
    use crate::kv::health::{WatchHealth, WatchHealthChannel};
    use crate::kv::key::KvKey;
    use crate::kv::supervisor::supervise;

    enum Attempt {
        Ends(Vec<&'static str>),
        Faults(Vec<&'static str>),
    }

    struct ScriptedSource {
        present: SyncMutex<VecDeque<bool>>,
        attempts: SyncMutex<VecDeque<Attempt>>,
        parked: Arc<Notify>,
    }

    impl ScriptedSource {
        fn new(present: Vec<bool>, attempts: Vec<Attempt>) -> Self {
            Self {
                present: SyncMutex::new(present.into()),
                attempts: SyncMutex::new(attempts.into()),
                parked: Arc::new(Notify::new()),
            }
        }

        fn deliver(
            keys: &[&'static str],
            on_change: &mut (dyn FnMut(EphemeralAuthChange<String>) + Send),
        ) {
            for key in keys {
                on_change(EphemeralAuthChange::Set {
                    key: KvKey::new(*key).unwrap(),
                    value: (*key).to_string(),
                });
            }
        }
    }

    #[async_trait::async_trait]
    impl ChangeSource for ScriptedSource {
        type Value = String;

        async fn bucket_present(&self) -> Result<(), FabricError> {
            let next = self.present.lock().unwrap().pop_front();
            match next {
                Some(true) => Ok(()),
                Some(false) => Err(FabricError::kv("bucket absent")),
                None => {
                    self.parked.notify_one();
                    std::future::pending().await
                }
            }
        }

        async fn follow_changes(
            &self,
            on_change: &mut (dyn FnMut(EphemeralAuthChange<String>) + Send),
        ) -> Result<(), FabricError> {
            let next = self.attempts.lock().unwrap().pop_front();
            match next {
                Some(Attempt::Ends(keys)) => {
                    Self::deliver(&keys, on_change);
                    Ok(())
                }
                Some(Attempt::Faults(keys)) => {
                    Self::deliver(&keys, on_change);
                    Err(FabricError::kv("watch stream broke"))
                }
                None => {
                    self.parked.notify_one();
                    std::future::pending().await
                }
            }
        }
    }

    async fn drive<H>(cycle: &SupervisedWatch<'_, ScriptedSource, H>, health: &WatchHealthChannel)
    where
        H: FnMut(EphemeralAuthChange<String>) + Send,
    {
        let parked = cycle.source.parked.clone();
        let backoff = Backoff::new(Duration::ZERO, Duration::ZERO);
        tokio::select! {
            _ = supervise(cycle, health, backoff, "ephemeral-auth") => unreachable!("supervise never returns"),
            _ = parked.notified() => {}
        }
    }

    #[tokio::test]
    async fn follow_reports_progress_when_the_attempt_delivered_a_change() {
        let source = ScriptedSource::new(vec![], vec![Attempt::Faults(vec!["auth/refresh/one"])]);
        let cycle = SupervisedWatch::new(&source, |_| {});

        let report = cycle.follow().await;

        assert!(report.progressed);
        assert!(report.outcome.is_err());
    }

    #[tokio::test]
    async fn follow_reports_no_progress_when_the_attempt_delivered_nothing() {
        let source = ScriptedSource::new(vec![], vec![Attempt::Ends(vec![])]);
        let cycle = SupervisedWatch::new(&source, |_| {});

        let report = cycle.follow().await;

        assert!(!report.progressed);
        assert!(report.outcome.is_ok());
    }

    #[tokio::test]
    async fn every_change_reaches_the_caller_handler() {
        let seen = Arc::new(SyncMutex::new(Vec::new()));
        let sink = seen.clone();
        let source = ScriptedSource::new(
            vec![],
            vec![Attempt::Ends(vec!["auth/refresh/one", "auth/refresh/two"])],
        );
        let cycle = SupervisedWatch::new(&source, move |change| sink.lock().unwrap().push(change));

        cycle.follow().await;

        assert_eq!(
            *seen.lock().unwrap(),
            vec![
                EphemeralAuthChange::Set {
                    key: KvKey::new("auth/refresh/one").unwrap(),
                    value: "auth/refresh/one".to_string(),
                },
                EphemeralAuthChange::Set {
                    key: KvKey::new("auth/refresh/two").unwrap(),
                    value: "auth/refresh/two".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn a_watch_fault_re_arms_the_follow_and_health_recovers_to_healthy() {
        let source = ScriptedSource::new(
            vec![true, true],
            vec![Attempt::Faults(vec!["auth/refresh/one"])],
        );
        let cycle = SupervisedWatch::new(&source, |_| {});
        let health = WatchHealthChannel::new();
        let rx = health.receiver();

        drive(&cycle, &health).await;

        assert!(source.attempts.lock().unwrap().is_empty());
        assert_eq!(*rx.borrow(), WatchHealth::Healthy);
    }

    #[tokio::test]
    async fn an_absent_bucket_holds_the_cycle_degraded_until_it_comes_back() {
        let source = ScriptedSource::new(vec![false, false, true], vec![]);
        let cycle = SupervisedWatch::new(&source, |_| {});
        let health = WatchHealthChannel::new();
        let rx = health.receiver();

        drive(&cycle, &health).await;

        assert!(source.present.lock().unwrap().is_empty());
        assert_eq!(*rx.borrow(), WatchHealth::Healthy);
    }

    #[tokio::test]
    async fn the_caller_handler_keeps_its_state_across_re_arms() {
        let calls = Arc::new(SyncMutex::new(Vec::new()));
        let sink = calls.clone();
        let mut nth = 0usize;
        let source = ScriptedSource::new(
            vec![true, true],
            vec![
                Attempt::Faults(vec!["auth/refresh/one"]),
                Attempt::Faults(vec!["auth/refresh/two"]),
            ],
        );
        let cycle = SupervisedWatch::new(&source, move |_| {
            nth += 1;
            sink.lock().unwrap().push(nth);
        });
        let health = WatchHealthChannel::new();

        drive(&cycle, &health).await;

        assert_eq!(*calls.lock().unwrap(), vec![1, 2]);
    }
}
