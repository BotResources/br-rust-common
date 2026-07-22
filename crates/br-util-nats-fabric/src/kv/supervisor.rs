use crate::consumer::backoff::Backoff;
use crate::kv::health::{WatchHealth, WatchHealthChannel};

pub(crate) struct FollowReport {
    pub(crate) progressed: bool,
    pub(crate) outcome: Result<(), String>,
}

#[async_trait::async_trait]
pub(crate) trait ReconcileCycle {
    async fn reconcile(&self) -> Result<(), String>;
    async fn follow(&self) -> FollowReport;
}

pub(crate) async fn supervise<C>(
    cycle: &C,
    health: &WatchHealthChannel,
    mut backoff: Backoff,
) -> std::convert::Infallible
where
    C: ReconcileCycle + ?Sized,
{
    health.set(WatchHealth::Degraded);
    loop {
        while let Err(err) = cycle.reconcile().await {
            health.set(WatchHealth::Degraded);
            let delay = backoff.sleep().await;
            tracing::warn!(
                error = %err,
                delay_ms = delay.as_millis() as u64,
                "published-language re-reconciliation failed; retrying"
            );
        }
        health.set(WatchHealth::Healthy);
        let report = cycle.follow().await;
        match &report.outcome {
            Ok(()) => {
                tracing::warn!(
                    "published-language watch stream ended; re-reconciling after backoff"
                )
            }
            Err(err) => tracing::warn!(
                error = %err,
                "published-language watch failed; re-reconciling after backoff"
            ),
        }
        if report.progressed {
            backoff.reset();
        }
        health.set(WatchHealth::Degraded);
        let delay = backoff.sleep().await;
        tracing::warn!(
            delay_ms = delay.as_millis() as u64,
            "backing off before re-bootstrap"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::sync::Notify;

    fn instant() -> Backoff {
        Backoff::new(Duration::ZERO, Duration::ZERO)
    }

    #[derive(Clone, Copy)]
    enum Boot {
        Ok,
        Err,
    }

    #[derive(Clone, Copy)]
    enum Follow {
        Ended { progressed: bool },
        Faulted { progressed: bool },
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        Reconcile(bool),
        Follow(bool),
    }

    struct ScriptedCycle {
        boots: Mutex<VecDeque<Boot>>,
        follows: Mutex<VecDeque<Follow>>,
        log: Arc<Mutex<Vec<Event>>>,
        parked: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl ReconcileCycle for ScriptedCycle {
        async fn reconcile(&self) -> Result<(), String> {
            let next = self.boots.lock().unwrap().pop_front();
            match next {
                Some(Boot::Ok) => {
                    self.log.lock().unwrap().push(Event::Reconcile(true));
                    Ok(())
                }
                Some(Boot::Err) => {
                    self.log.lock().unwrap().push(Event::Reconcile(false));
                    Err("reconcile failed".to_string())
                }
                None => {
                    self.parked.notify_one();
                    std::future::pending().await
                }
            }
        }

        async fn follow(&self) -> FollowReport {
            let next = self.follows.lock().unwrap().pop_front();
            match next {
                Some(Follow::Ended { progressed }) => {
                    self.log.lock().unwrap().push(Event::Follow(true));
                    FollowReport {
                        progressed,
                        outcome: Ok(()),
                    }
                }
                Some(Follow::Faulted { progressed }) => {
                    self.log.lock().unwrap().push(Event::Follow(false));
                    FollowReport {
                        progressed,
                        outcome: Err("watch failed".to_string()),
                    }
                }
                None => {
                    self.parked.notify_one();
                    std::future::pending().await
                }
            }
        }
    }

    fn scripted(
        boots: Vec<Boot>,
        follows: Vec<Follow>,
    ) -> (ScriptedCycle, Arc<Mutex<Vec<Event>>>, Arc<Notify>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let parked = Arc::new(Notify::new());
        let cycle = ScriptedCycle {
            boots: Mutex::new(boots.into()),
            follows: Mutex::new(follows.into()),
            log: log.clone(),
            parked: parked.clone(),
        };
        (cycle, log, parked)
    }

    async fn drive(cycle: &ScriptedCycle, health: &WatchHealthChannel, backoff: Backoff) {
        let parked = cycle.parked.clone();
        tokio::select! {
            _ = supervise(cycle, health, backoff) => unreachable!("supervise never returns"),
            _ = parked.notified() => {}
        }
    }

    #[tokio::test]
    async fn a_watch_fault_triggers_a_rebootstrap_and_health_recovers_to_healthy() {
        let (cycle, log, _) = scripted(
            vec![Boot::Ok, Boot::Ok],
            vec![Follow::Faulted { progressed: true }],
        );
        let health = WatchHealthChannel::new();
        let rx = health.receiver();

        drive(&cycle, &health, instant()).await;

        assert_eq!(
            *log.lock().unwrap(),
            vec![
                Event::Reconcile(true),
                Event::Follow(false),
                Event::Reconcile(true),
            ]
        );
        assert_eq!(*rx.borrow(), WatchHealth::Healthy);
    }

    #[tokio::test]
    async fn a_clean_stream_end_is_also_treated_as_a_fault_and_re_reconciled() {
        let (cycle, log, _) = scripted(
            vec![Boot::Ok, Boot::Ok],
            vec![Follow::Ended { progressed: true }],
        );
        let health = WatchHealthChannel::new();

        drive(&cycle, &health, instant()).await;

        assert_eq!(
            *log.lock().unwrap(),
            vec![
                Event::Reconcile(true),
                Event::Follow(true),
                Event::Reconcile(true),
            ]
        );
    }

    #[tokio::test]
    async fn bootstrap_retries_until_success_and_health_is_healthy_only_afterwards() {
        let (cycle, log, _) = scripted(vec![Boot::Err, Boot::Err, Boot::Ok], vec![]);
        let health = WatchHealthChannel::new();
        let rx = health.receiver();

        drive(&cycle, &health, instant()).await;

        assert_eq!(
            *log.lock().unwrap(),
            vec![
                Event::Reconcile(false),
                Event::Reconcile(false),
                Event::Reconcile(true),
            ]
        );
        assert_eq!(*rx.borrow(), WatchHealth::Healthy);
    }

    #[tokio::test]
    async fn health_starts_degraded_before_the_first_bootstrap_completes() {
        let (cycle, log, _) = scripted(vec![], vec![]);
        let health = WatchHealthChannel::new();
        let rx = health.receiver();

        drive(&cycle, &health, instant()).await;

        assert!(log.lock().unwrap().is_empty());
        assert_eq!(*rx.borrow(), WatchHealth::Degraded);
    }

    #[tokio::test]
    async fn a_bootstrap_failure_is_observable_as_degraded() {
        let (cycle, log, _) = scripted(vec![Boot::Err], vec![]);
        let health = WatchHealthChannel::new();
        let rx = health.receiver();

        drive(&cycle, &health, instant()).await;

        assert_eq!(*log.lock().unwrap(), vec![Event::Reconcile(false)]);
        assert_eq!(*rx.borrow(), WatchHealth::Degraded);
    }

    #[tokio::test(start_paused = true)]
    async fn an_instant_watch_flap_escalates_the_backoff() {
        let boots = vec![Boot::Ok; 5];
        let follows = vec![Follow::Faulted { progressed: false }; 5];
        let (cycle, _, _) = scripted(boots, follows);
        let health = WatchHealthChannel::new();
        let backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(10));

        let start = tokio::time::Instant::now();
        drive(&cycle, &health, backoff).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(3100),
            "a 0-entry watch flap must escalate (100+200+400+800+1600), got {elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn a_watch_that_delivered_entries_resets_the_backoff_floor() {
        let boots = vec![Boot::Ok; 5];
        let follows = vec![Follow::Faulted { progressed: true }; 5];
        let (cycle, _, _) = scripted(boots, follows);
        let health = WatchHealthChannel::new();
        let backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(10));

        let start = tokio::time::Instant::now();
        drive(&cycle, &health, backoff).await;
        let elapsed = start.elapsed();

        assert!(
            (Duration::from_millis(500)..Duration::from_millis(700)).contains(&elapsed),
            "real progress must reset the floor to 100ms/cycle (5x100), got {elapsed:?}"
        );
    }
}
