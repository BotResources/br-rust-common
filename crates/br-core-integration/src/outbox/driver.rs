use std::time::Duration;

use sqlx::postgres::PgListener;
use tokio::sync::watch;

use crate::outbox::health::{REASON_NO_STREAM, RelayHealth};
use crate::outbox::relay::OutboxRelay;
use crate::outbox::report::RelayReport;
use crate::outbox::retry::retry_backoff;
use crate::outbox::store::OutboxStoreError;

#[derive(Debug)]
#[non_exhaustive]
pub enum RelayRunError {
    Listener(sqlx::Error),
    Store(OutboxStoreError),
}

impl std::fmt::Display for RelayRunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Listener(e) => write!(f, "outbox relay listener failed: {e}"),
            Self::Store(e) => write!(f, "outbox relay drain failed: {e}"),
        }
    }
}

impl std::error::Error for RelayRunError {}

impl From<OutboxStoreError> for RelayRunError {
    fn from(e: OutboxStoreError) -> Self {
        Self::Store(e)
    }
}

impl OutboxRelay {
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<(), RelayRunError> {
        let mut listener = PgListener::connect_with(&self.pool)
            .await
            .map_err(RelayRunError::Listener)?;
        listener
            .listen(&self.store.notify_channel())
            .await
            .map_err(RelayRunError::Listener)?;

        let mut next_retry = self.drain_and_update_health().await?;

        loop {
            let retry_sleep = next_retry.map(tokio::time::sleep);

            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                recv = listener.try_recv() => {
                    recv.map_err(RelayRunError::Listener)?;
                    next_retry = self.drain_and_update_health().await?;
                }
                () = maybe_sleep(retry_sleep) => {
                    next_retry = self.drain_and_update_health().await?;
                }
            }
        }
    }

    async fn drain_and_update_health(&self) -> Result<Option<Duration>, OutboxStoreError> {
        let report = self.run_once().await?;
        self.update_health(&report);
        Ok(next_retry_delay(&report))
    }

    fn update_health(&self, report: &RelayReport) {
        if report.structural > 0 {
            self.health.set(RelayHealth::Degraded {
                reason: REASON_NO_STREAM,
            });
        } else {
            self.health.set(RelayHealth::Healthy);
        }
    }
}

fn next_retry_delay(report: &RelayReport) -> Option<Duration> {
    report.min_retry_attempts.map(retry_backoff)
}

async fn maybe_sleep(sleep: Option<tokio::time::Sleep>) {
    match sleep {
        Some(s) => s.await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report_with(structural: usize, min_retry_attempts: Option<u32>) -> RelayReport {
        RelayReport {
            structural,
            min_retry_attempts,
            ..RelayReport::default()
        }
    }

    #[test]
    fn no_retry_owed_yields_no_delay() {
        assert_eq!(next_retry_delay(&report_with(0, None)), None);
    }

    #[test]
    fn a_transient_retry_yields_its_backoff() {
        let delay = next_retry_delay(&report_with(0, Some(1))).expect("a delay");
        assert_eq!(delay, retry_backoff(1));
    }

    #[test]
    fn a_structural_failure_does_not_owe_a_retry_delay() {
        assert_eq!(next_retry_delay(&report_with(2, None)), None);
    }
}
