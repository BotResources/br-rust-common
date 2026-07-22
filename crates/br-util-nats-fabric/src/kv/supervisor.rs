use crate::consumer::backoff::Backoff;
use crate::kv::health::{WatchHealth, WatchHealthChannel};

#[async_trait::async_trait]
pub(crate) trait ReconcileCycle {
    async fn reconcile(&self) -> Result<(), String>;
    async fn follow(&self) -> Result<(), String>;
}

pub(crate) async fn supervise<C>(cycle: &C, health: &WatchHealthChannel, mut backoff: Backoff)
where
    C: ReconcileCycle + ?Sized,
{
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
        backoff.reset();
        health.set(WatchHealth::Healthy);
        match cycle.follow().await {
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
    enum Outcome {
        Ok,
        Err,
    }

    #[derive(Debug, PartialEq, Eq)]
    enum Event {
        Reconcile(bool),
        Follow(bool),
    }

    struct ScriptedCycle {
        reconciles: Mutex<VecDeque<Outcome>>,
        follows: Mutex<VecDeque<Outcome>>,
        log: Arc<Mutex<Vec<Event>>>,
        parked: Arc<Notify>,
    }

    #[async_trait::async_trait]
    impl ReconcileCycle for ScriptedCycle {
        async fn reconcile(&self) -> Result<(), String> {
            let next = self.reconciles.lock().unwrap().pop_front();
            match next {
                Some(Outcome::Ok) => {
                    self.log.lock().unwrap().push(Event::Reconcile(true));
                    Ok(())
                }
                Some(Outcome::Err) => {
                    self.log.lock().unwrap().push(Event::Reconcile(false));
                    Err("reconcile failed".to_string())
                }
                None => {
                    self.parked.notify_one();
                    std::future::pending().await
                }
            }
        }

        async fn follow(&self) -> Result<(), String> {
            let next = self.follows.lock().unwrap().pop_front();
            match next {
                Some(Outcome::Ok) => {
                    self.log.lock().unwrap().push(Event::Follow(true));
                    Ok(())
                }
                Some(Outcome::Err) => {
                    self.log.lock().unwrap().push(Event::Follow(false));
                    Err("watch failed".to_string())
                }
                None => {
                    self.parked.notify_one();
                    std::future::pending().await
                }
            }
        }
    }

    fn scripted(
        reconciles: Vec<Outcome>,
        follows: Vec<Outcome>,
    ) -> (ScriptedCycle, Arc<Mutex<Vec<Event>>>, Arc<Notify>) {
        let log = Arc::new(Mutex::new(Vec::new()));
        let parked = Arc::new(Notify::new());
        let cycle = ScriptedCycle {
            reconciles: Mutex::new(reconciles.into()),
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
        let (cycle, log, _) = scripted(vec![Outcome::Ok, Outcome::Ok], vec![Outcome::Err]);
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
        let (cycle, log, _) = scripted(vec![Outcome::Ok, Outcome::Ok], vec![Outcome::Ok]);
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
        let (cycle, log, _) = scripted(vec![Outcome::Err, Outcome::Err, Outcome::Ok], vec![]);
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
    async fn a_bootstrap_failure_is_observable_as_degraded() {
        let (cycle, log, _) = scripted(vec![Outcome::Err], vec![]);
        let health = WatchHealthChannel::new();
        let rx = health.receiver();

        drive(&cycle, &health, instant()).await;

        assert_eq!(*log.lock().unwrap(), vec![Event::Reconcile(false)]);
        assert_eq!(*rx.borrow(), WatchHealth::Degraded);
    }

    #[tokio::test(start_paused = true)]
    async fn consecutive_bootstrap_failures_back_off_exponentially() {
        let (cycle, _, _) = scripted(
            vec![Outcome::Err, Outcome::Err, Outcome::Err, Outcome::Ok],
            vec![],
        );
        let health = WatchHealthChannel::new();
        let backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(1));

        let start = tokio::time::Instant::now();
        drive(&cycle, &health, backoff).await;
        let elapsed = start.elapsed();

        assert!(
            elapsed >= Duration::from_millis(700),
            "expected >=700ms cumulative backoff (100+200+400), got {elapsed:?}"
        );
    }
}
