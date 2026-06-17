use std::time::Duration;

use sqlx::postgres::PgListener;
use tokio::sync::watch;

use br_core_integration::retry_backoff;

use crate::outbox::health::{REASON_NO_STREAM, RelayHealth};
use crate::outbox::relay::OutboxRelay;
use crate::outbox::report::RelayReport;
use crate::outbox::store::{OUTBOX_NOTIFY_CHANNEL, OutboxStoreError};

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
            .listen(OUTBOX_NOTIFY_CHANNEL)
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
        Ok(schedule_after(&report, self.policy.max_messages.max(1)))
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

fn schedule_after(report: &RelayReport, cap: usize) -> Option<Duration> {
    let saturated = report.picked >= cap;
    let made_progress = report.published > 0 || report.failed > 0;
    if saturated && made_progress {
        return Some(Duration::ZERO);
    }
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

    fn report_with(
        picked: usize,
        published: usize,
        failed: usize,
        min_retry_attempts: Option<u32>,
    ) -> RelayReport {
        RelayReport {
            picked,
            published,
            failed,
            min_retry_attempts,
            ..RelayReport::default()
        }
    }

    const CAP: usize = 256;

    #[test]
    fn saturated_with_progress_redrains_immediately() {
        assert_eq!(
            schedule_after(&report_with(CAP, CAP, 0, None), CAP),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn saturated_with_progress_and_a_retry_owed_still_redrains_immediately() {
        assert_eq!(
            schedule_after(&report_with(CAP, CAP - 1, 0, Some(1)), CAP),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn saturated_with_terminal_failures_redrains_immediately() {
        assert_eq!(
            schedule_after(&report_with(CAP, 0, CAP, None), CAP),
            Some(Duration::ZERO)
        );
    }

    #[test]
    fn saturated_with_no_progress_does_not_redrain() {
        assert_eq!(schedule_after(&report_with(CAP, 0, 0, None), CAP), None);
    }

    #[test]
    fn saturated_all_transient_honors_backoff() {
        let delay = schedule_after(&report_with(CAP, 0, 0, Some(1)), CAP).expect("a delay");
        assert_eq!(delay, retry_backoff(1));
    }

    #[test]
    fn not_saturated_with_no_retry_owed_yields_no_delay() {
        assert_eq!(
            schedule_after(&report_with(CAP - 1, CAP - 1, 0, None), CAP),
            None
        );
    }

    #[test]
    fn not_saturated_with_a_transient_retry_yields_its_backoff() {
        let delay =
            schedule_after(&report_with(CAP - 1, CAP - 2, 0, Some(1)), CAP).expect("a delay");
        assert_eq!(delay, retry_backoff(1));
    }
}
