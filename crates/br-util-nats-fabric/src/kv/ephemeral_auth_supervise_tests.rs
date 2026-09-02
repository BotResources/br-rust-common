use std::collections::VecDeque;
use std::sync::{Arc, Mutex as SyncMutex};
use std::time::Duration;
use tokio::sync::Notify;

use crate::consumer::backoff::Backoff;
use crate::error::FabricError;
use crate::kv::ephemeral_auth_supervise::{ChangeSource, SupervisedWatch};
use crate::kv::ephemeral_auth_watch::EphemeralAuthChange;
use crate::kv::health::{WatchHealth, WatchHealthChannel};
use crate::kv::key::KvKey;
use crate::kv::supervisor::{ReconcileCycle, supervise};

enum Attempt {
    Ends(Vec<&'static str>),
    Faults(Vec<&'static str>),
    SkipsThenFaults(u64),
}

struct ScriptedSource {
    present: SyncMutex<VecDeque<bool>>,
    attempts: SyncMutex<VecDeque<Attempt>>,
    observed: SyncMutex<u64>,
    parked: Arc<Notify>,
}

impl ScriptedSource {
    fn new(present: Vec<bool>, attempts: Vec<Attempt>) -> Self {
        Self {
            present: SyncMutex::new(present.into()),
            attempts: SyncMutex::new(attempts.into()),
            observed: SyncMutex::new(0),
            parked: Arc::new(Notify::new()),
        }
    }

    fn deliver(
        &self,
        keys: &[&'static str],
        on_change: &mut (dyn FnMut(EphemeralAuthChange<String>) + Send),
    ) {
        for key in keys {
            *self.observed.lock().unwrap() += 1;
            on_change(EphemeralAuthChange::Set {
                key: KvKey::new(*key).unwrap(),
                value: (*key).to_string(),
            });
        }
    }

    fn skip(&self, count: u64) {
        *self.observed.lock().unwrap() += count;
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

    fn observed(&self) -> u64 {
        *self.observed.lock().unwrap()
    }

    async fn follow_changes(
        &self,
        on_change: &mut (dyn FnMut(EphemeralAuthChange<String>) + Send),
    ) -> Result<(), FabricError> {
        let next = self.attempts.lock().unwrap().pop_front();
        match next {
            Some(Attempt::Ends(keys)) => {
                self.deliver(&keys, on_change);
                Ok(())
            }
            Some(Attempt::Faults(keys)) => {
                self.deliver(&keys, on_change);
                Err(FabricError::kv("watch stream broke"))
            }
            Some(Attempt::SkipsThenFaults(count)) => {
                self.skip(count);
                Err(FabricError::kv("watch stream broke"))
            }
            None => {
                self.parked.notify_one();
                std::future::pending().await
            }
        }
    }
}

fn instant() -> Backoff {
    Backoff::new(Duration::ZERO, Duration::ZERO)
}

async fn drive<H>(
    source: &ScriptedSource,
    cycle: &SupervisedWatch<'_, ScriptedSource, H>,
    health: &WatchHealthChannel,
    backoff: Backoff,
) where
    H: FnMut(EphemeralAuthChange<String>) + Send,
{
    let parked = source.parked.clone();
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

    drive(&source, &cycle, &health, instant()).await;

    assert!(source.attempts.lock().unwrap().is_empty());
    assert_eq!(*rx.borrow(), WatchHealth::Healthy);
}

#[tokio::test]
async fn an_absent_bucket_holds_the_cycle_degraded_until_it_comes_back() {
    let source = ScriptedSource::new(vec![false, false, true], vec![]);
    let cycle = SupervisedWatch::new(&source, |_| {});
    let health = WatchHealthChannel::new();
    let rx = health.receiver();

    drive(&source, &cycle, &health, instant()).await;

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

    drive(&source, &cycle, &health, instant()).await;

    assert_eq!(*calls.lock().unwrap(), vec![1, 2]);
}

#[tokio::test]
async fn an_attempt_that_only_skipped_unreadable_entries_still_reports_progress() {
    let source = ScriptedSource::new(vec![], vec![Attempt::SkipsThenFaults(3)]);
    let cycle = SupervisedWatch::new(&source, |_| {});

    let report = cycle.follow().await;

    assert!(
        report.progressed,
        "entries the watch read but could not decode are progress: the stream is alive"
    );
    assert!(report.outcome.is_err());
}

#[tokio::test(start_paused = true)]
async fn a_watch_that_only_skipped_entries_resets_the_backoff_floor() {
    let boots = vec![true; 5];
    let attempts = (0..5).map(|_| Attempt::SkipsThenFaults(1)).collect();
    let source = ScriptedSource::new(boots, attempts);
    let cycle = SupervisedWatch::new(&source, |_| {});
    let health = WatchHealthChannel::new();
    let backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(10));

    let start = tokio::time::Instant::now();
    drive(&source, &cycle, &health, backoff).await;
    let elapsed = start.elapsed();

    assert!(
        (Duration::from_millis(500)..Duration::from_millis(700)).contains(&elapsed),
        "a stream delivering only unreadable entries must not escalate to the cap, got {elapsed:?}"
    );
}
