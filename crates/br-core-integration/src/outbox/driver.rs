//! The relay's **subscribe-driven** run loop — [`OutboxRelay::run`].
//!
//! This is the half that replaces the old "call `run_once` on a timer". The
//! relay never reads the outbox on a blind clock. Instead it:
//!
//! 1. does **one** startup recovery drain (the crash-recovery sweep — rows a
//!    crash left `Pending`);
//! 2. then parks on a `tokio::select!` that wakes only on an actual event:
//!    - a Postgres `LISTEN`/`NOTIFY` on the store's channel, fired by `stage` at
//!      commit → drain-until-empty;
//!    - a listener **reconnect** (`PgListener::try_recv` returns `Ok(None)` when
//!      the connection was lost and re-established) → drain, covering the one
//!      window where a `NOTIFY` could have been missed while the socket was down;
//!    - a chained **retry deadline**, present in the select *only* when a
//!      transient failure left a row owed a retry → re-drain;
//!    - shutdown → break.
//!
//! When the outbox is clean and no retry is owed, the select is over
//! {notification, shutdown} only — the relay is genuinely **parked at zero CPU**
//! and issues **zero** DB traffic until the next `NOTIFY`.

use std::time::Duration;

use sqlx::postgres::PgListener;
use tokio::sync::watch;

use crate::outbox::health::{REASON_NO_STREAM, RelayHealth};
use crate::outbox::relay::OutboxRelay;
use crate::outbox::report::RelayReport;
use crate::outbox::retry::retry_backoff;
use crate::outbox::store::OutboxStoreError;

/// Why [`OutboxRelay::run`] returned — a clean shutdown, or a fatal error it
/// could not continue past (the listener could not be established, or a drain
/// pass hit a store error such as a missing table). A transient publish failure
/// is **not** fatal — it stays inside the loop as a retry.
#[derive(Debug)]
#[non_exhaustive]
pub enum RelayRunError {
    /// The Postgres `LISTEN` connection could not be established or was lost
    /// unrecoverably. Fail loud — the relay cannot be woken without it.
    Listener(sqlx::Error),
    /// A drain pass failed against the store (e.g. the declared table is
    /// missing, or the row status is corrupt). Fail loud.
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
    /// Run the subscribe-driven relay until `shutdown` flips to `true`.
    ///
    /// `shutdown` is a [`watch::Receiver<bool>`]: the caller holds the matching
    /// `Sender` and sends `true` (or drops it) to ask the loop to stop after the
    /// current drain. A `watch` is chosen over a one-shot so the same signal can
    /// fan out to several tasks, and over a `CancellationToken` to avoid a
    /// `tokio-util` dependency in a `core` crate.
    ///
    /// On entry it does one [`run_once`](Self::run_once) recovery drain, then
    /// loops on the `select!` over {`NOTIFY`/reconnect, retry deadline,
    /// shutdown}. It returns `Ok(())` on a clean shutdown, or a [`RelayRunError`]
    /// on a fatal listener or store fault (a transient publish failure is handled
    /// inside the loop as a retry, never surfaced here).
    pub async fn run(&self, mut shutdown: watch::Receiver<bool>) -> Result<(), RelayRunError> {
        // Establish the LISTEN before the recovery drain, so a NOTIFY fired
        // between the drain and entering the select! is buffered, not missed.
        let mut listener = PgListener::connect_with(&self.pool)
            .await
            .map_err(RelayRunError::Listener)?;
        listener
            .listen(&self.store.notify_channel())
            .await
            .map_err(RelayRunError::Listener)?;

        // Startup recovery sweep: drain whatever a crash left Pending.
        let mut next_retry = self.drain_and_update_health().await?;

        loop {
            // The retry arm exists ONLY when a transient failure owes a retry;
            // otherwise the select is over {notification, shutdown} and parks.
            let retry_sleep = next_retry.map(tokio::time::sleep);

            tokio::select! {
                // Shutdown requested (sent true, or the sender dropped → recv err).
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return Ok(());
                    }
                }
                // A NOTIFY arrived, or the listener reconnected (Ok(None)). Both
                // mean "drain now": a fresh row was committed, or a reconnect may
                // have hidden a NOTIFY while the socket was down.
                recv = listener.try_recv() => {
                    recv.map_err(RelayRunError::Listener)?;
                    next_retry = self.drain_and_update_health().await?;
                }
                // The chained retry deadline fired (present only when owed).
                () = maybe_sleep(retry_sleep) => {
                    next_retry = self.drain_and_update_health().await?;
                }
            }
        }
    }

    /// Run one drain pass, update the health signal from its structural count,
    /// and return the next chained retry delay (if a transient failure left a
    /// row owed a retry).
    async fn drain_and_update_health(&self) -> Result<Option<Duration>, OutboxStoreError> {
        let report = self.run_once().await?;
        self.update_health(&report);
        Ok(next_retry_delay(&report))
    }

    /// Map a pass's structural count to the health signal: any structural row →
    /// `Degraded`; a clean pass → `Healthy` (recovery).
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

/// The chained retry delay after a pass: the backoff for the soonest row left
/// `Pending` by a transient failure, or `None` if none were (no retry owed → the
/// select drops the deadline arm and parks).
fn next_retry_delay(report: &RelayReport) -> Option<Duration> {
    report.min_retry_attempts.map(retry_backoff)
}

/// Await an optional sleep: if `None` (no retry owed), never resolves, so the
/// `select!` arm is effectively absent and the loop parks on the other arms.
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

    // GIVEN a pass with no transient retry owed WHEN the delay is computed
    // THEN there is none (the select parks, no deadline arm)
    #[test]
    fn no_retry_owed_yields_no_delay() {
        assert_eq!(next_retry_delay(&report_with(0, None)), None);
    }

    // GIVEN a pass that left a transient row owed a retry WHEN computed
    // THEN the backoff for that row's attempt count is returned
    #[test]
    fn a_transient_retry_yields_its_backoff() {
        let delay = next_retry_delay(&report_with(0, Some(1))).expect("a delay");
        assert_eq!(delay, retry_backoff(1));
    }

    // GIVEN a structural-only pass WHEN the delay is computed THEN none is owed —
    // a structural row is re-driven by the next wake, not a budgeted retry.
    #[test]
    fn a_structural_failure_does_not_owe_a_retry_delay() {
        assert_eq!(next_retry_delay(&report_with(2, None)), None);
    }
}
