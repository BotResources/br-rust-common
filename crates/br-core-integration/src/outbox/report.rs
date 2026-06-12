//! The relay's pass **policy** and **report** — the tuning a pass reads and the
//! tally it produces, plus the pure per-row classification that fills the tally.
//!
//! Kept out of `relay.rs` so that file stays the drain itself. The tally drives
//! both metering and the [`run`](super::OutboxRelay::run) loop (health from
//! `structural`, the retry deadline from `min_retry_attempts`), so the
//! classification is unit-tested here as a spec.

use crate::outbox::status::{OutboxStatus, Transition};

/// How many publish attempts the relay makes across passes before marking a row
/// `Failed`. Counts attempts recorded on the row, not retries within one pass.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

/// How many `Pending` rows one [`OutboxRelay::run_once`](super::OutboxRelay::run_once)
/// pass processes before returning — a per-invocation cap that bounds a single
/// pass even if rows keep arriving. Each row is its own short transaction.
pub const DEFAULT_MAX_MESSAGES: usize = 256;

/// Tuning for an [`OutboxRelay`](super::OutboxRelay) pass.
///
/// `#[non_exhaustive]`: start from [`RelayPolicy::default`] and override fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelayPolicy {
    /// Attempts (across passes) before a row is marked `Failed`. Clamped to ≥1.
    pub max_attempts: u32,
    /// Max rows one pass processes — each in its own short transaction — before
    /// it returns. Bounds a single invocation. Clamped to ≥1.
    pub max_messages: usize,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            max_messages: DEFAULT_MAX_MESSAGES,
        }
    }
}

/// Outcome of one [`OutboxRelay::run_once`](super::OutboxRelay::run_once) pass —
/// what the caller logs / meters and what the [`run`](super::OutboxRelay::run)
/// loop uses to drive health and the retry deadline.
///
/// One pass processes rows one at a time (each its own short transaction). The
/// publish-outcome counts sum the per-row results: `picked == published + failed
/// + retried + structural`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelayReport {
    /// Rows picked up and processed this pass (each in its own transaction).
    pub picked: usize,
    /// Rows that reached `Published` this pass.
    pub published: usize,
    /// Rows that reached the terminal `Failed` state this pass.
    pub failed: usize,
    /// Rows whose **transient** publish failed but stay `Pending` for a later
    /// pass (an attempt was consumed).
    pub retried: usize,
    /// Rows whose publish hit a **structural** fault (the target stream is not
    /// declared): they stay `Pending` with **no** attempt consumed. A non-zero
    /// count drives the relay's health to `Degraded`.
    pub structural: usize,
    /// The smallest attempt count among rows left `Pending` by a *transient*
    /// failure this pass, or `None` if none were. The
    /// [`run`](super::OutboxRelay::run) loop derives the chained retry-deadline
    /// backoff from it (the soonest row to retry). Structural rows do not
    /// contribute — they are re-driven by the next wake, not by a budgeted retry.
    pub min_retry_attempts: Option<u32>,
}

/// Update the running [`RelayReport`] for one row's outcome, branching on whether
/// the failure was structural (no attempt consumed) vs transient (drives retry).
pub(super) fn classify_pass(
    report: &mut RelayReport,
    publish_result: &Result<(), crate::IntegrationError>,
    transition: Transition,
    structural: bool,
) {
    use OutboxStatus::{Failed, Published};
    if publish_result.is_ok() {
        report.published += 1;
        return;
    }
    if structural {
        report.structural += 1;
        return;
    }
    // Transient failure: record the retry and track the soonest row to retry.
    if transition.status == Failed {
        report.failed += 1;
    } else {
        report.retried += 1;
        report.min_retry_attempts = Some(match report.min_retry_attempts {
            Some(prev) => prev.min(transition.attempts),
            None => transition.attempts,
        });
    }
    // `Published` only ever appears on success (handled above); the explicit
    // import keeps the match exhaustive-by-name without a wildcard.
    let _ = Published;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PublishErrorKind;

    fn no_stream() -> Result<(), crate::IntegrationError> {
        Err(crate::IntegrationError::Publish {
            kind: PublishErrorKind::NoStream,
            detail: "no stream".into(),
        })
    }

    fn transient() -> Result<(), crate::IntegrationError> {
        Err(crate::IntegrationError::Publish {
            kind: PublishErrorKind::Timeout,
            detail: "timed out".into(),
        })
    }

    // GIVEN the default policy WHEN inspected THEN it carries the documented caps
    #[test]
    fn default_policy_has_documented_caps() {
        let p = RelayPolicy::default();
        assert_eq!(p.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(p.max_messages, DEFAULT_MAX_MESSAGES);
    }

    // GIVEN a successful publish WHEN the pass is classified THEN it counts as published
    #[test]
    fn classify_counts_a_success_as_published() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Published,
            attempts: 1,
        };
        classify_pass(&mut report, &Ok(()), t, false);
        assert_eq!(report.published, 1);
        assert_eq!(report.retried, 0);
        assert_eq!(report.structural, 0);
        assert_eq!(report.min_retry_attempts, None);
    }

    // GIVEN a transient failure with retries left WHEN classified
    // THEN it counts as retried and tracks the attempt count for the backoff
    #[test]
    fn classify_counts_a_transient_retry_and_tracks_attempts() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Pending,
            attempts: 2,
        };
        classify_pass(&mut report, &transient(), t, false);
        assert_eq!(report.retried, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(report.structural, 0);
        assert_eq!(report.min_retry_attempts, Some(2));
    }

    // GIVEN two transient retries WHEN classified THEN min_retry_attempts is the
    // smaller (the soonest row the loop should retry)
    #[test]
    fn min_retry_attempts_tracks_the_soonest() {
        let mut report = RelayReport::default();
        classify_pass(
            &mut report,
            &transient(),
            Transition {
                status: OutboxStatus::Pending,
                attempts: 3,
            },
            false,
        );
        classify_pass(
            &mut report,
            &transient(),
            Transition {
                status: OutboxStatus::Pending,
                attempts: 1,
            },
            false,
        );
        assert_eq!(report.min_retry_attempts, Some(1));
    }

    // GIVEN a transient failure at the cap WHEN classified THEN it counts as failed
    // (terminal — does not drive a further retry)
    #[test]
    fn classify_counts_a_terminal_failure() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Failed,
            attempts: 5,
        };
        classify_pass(&mut report, &transient(), t, false);
        assert_eq!(report.failed, 1);
        assert_eq!(report.retried, 0);
        assert_eq!(report.min_retry_attempts, None);
    }

    // GIVEN a structural failure WHEN classified THEN it counts as structural and
    // does NOT consume the retry budget (no attempt tracked, not failed)
    #[test]
    fn classify_counts_a_structural_failure_without_burning_retry() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Pending,
            attempts: 0,
        };
        classify_pass(&mut report, &no_stream(), t, true);
        assert_eq!(report.structural, 1);
        assert_eq!(report.retried, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(report.min_retry_attempts, None);
    }
}
