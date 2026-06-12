//! The outbox **relay**: the post-commit / crash-recovery sweep that publishes
//! `Pending` rows and persists their transition. Feature-gated behind `outbox`.
//!
//! The relay is the half of the outbox that makes "losing a critical
//! integration event is impossible" true. `stage` only guarantees the row is
//! durable *with* the domain write; the relay guarantees it is *published* — and
//! because it reads from the table rather than from an in-memory hand-off, the
//! exact same code that publishes right after a commit also re-publishes a row a
//! crash left behind (it was committed `Pending`, never published). There is no
//! separate recovery path to forget to run.
//!
//! ## Semantics — at-least-once, post-commit
//!
//! Publish happens *after* the staging transaction commits, so a consumer can
//! observe an event whose producer-side transaction is fully durable (no
//! dirty-read of an uncommitted write). The relay is **at-least-once**: a crash
//! after the broker ack but before `apply_transition` leaves the row `Pending`,
//! and the next pass re-publishes it. Subscribers must therefore de-dupe on the
//! envelope id (the same idempotency rule the consumer shapes already document).

use std::sync::Arc;

use crate::outbox::status::next_after_attempt;
use crate::outbox::store::{OutboxStore, OutboxStoreError};
use crate::{ConsumeErrorKind, IntegrationError, IntegrationPublisher};

/// How many publish attempts the relay makes across passes before marking a row
/// `Failed`. Counts attempts recorded on the row, not retries within one pass.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

/// How many `Pending` rows one [`OutboxRelay::run_once`] pass drains.
pub const DEFAULT_BATCH_SIZE: i64 = 64;

/// Tuning for an [`OutboxRelay`] pass.
///
/// `#[non_exhaustive]`: start from [`RelayPolicy::default`] and override fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelayPolicy {
    /// Attempts (across passes) before a row is marked `Failed`. Clamped to ≥1.
    pub max_attempts: u32,
    /// Rows drained per [`OutboxRelay::run_once`] pass.
    pub batch_size: i64,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            batch_size: DEFAULT_BATCH_SIZE,
        }
    }
}

/// Outcome of one [`OutboxRelay::run_once`] pass — what the caller logs / meters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelayReport {
    /// Rows picked up this pass.
    pub picked: usize,
    /// Rows that reached `Published` this pass.
    pub published: usize,
    /// Rows that reached the terminal `Failed` state this pass.
    pub failed: usize,
    /// Rows whose publish failed but stay `Pending` for a later pass.
    pub retried: usize,
}

/// Drains the outbox: reads `Pending` rows, publishes each through the shared
/// [`IntegrationPublisher`], and persists the transition the pure state machine
/// decides.
///
/// Holds a `pool` (its own connection source — the relay runs *after* the
/// caller's transaction, on its own transactions) and an `Arc<dyn
/// IntegrationPublisher>` (the same publisher the rest of the service uses, so a
/// `Noop` publisher makes the relay a no-op in tests).
pub struct OutboxRelay {
    pool: sqlx::PgPool,
    store: OutboxStore,
    publisher: Arc<dyn IntegrationPublisher>,
    policy: RelayPolicy,
}

impl OutboxRelay {
    /// A relay over the default table with the default policy.
    pub fn new(pool: sqlx::PgPool, publisher: Arc<dyn IntegrationPublisher>) -> Self {
        Self::with(
            pool,
            OutboxStore::default(),
            publisher,
            RelayPolicy::default(),
        )
    }

    /// A relay with an explicit store (table name) and policy.
    pub fn with(
        pool: sqlx::PgPool,
        store: OutboxStore,
        publisher: Arc<dyn IntegrationPublisher>,
        policy: RelayPolicy,
    ) -> Self {
        Self {
            pool,
            store,
            publisher,
            policy,
        }
    }

    /// Run one pass: pick a batch of `Pending` rows (locked `FOR UPDATE SKIP
    /// LOCKED`, so concurrent relay replicas drain disjoint batches), publish
    /// each, and persist its transition — all within one transaction so the lock
    /// is held until the transition is durable. Returns a [`RelayReport`].
    ///
    /// Call this on a schedule (a timer task) *and* once at startup: the startup
    /// pass is the crash-recovery sweep. Idle when the outbox is empty — it does
    /// not spin; the caller owns the interval.
    pub async fn run_once(&self) -> Result<RelayReport, OutboxStoreError> {
        let mut tx = self.pool.begin().await?;
        let pending = self
            .store
            .fetch_pending(&mut *tx, self.policy.batch_size)
            .await?;

        let mut report = RelayReport {
            picked: pending.len(),
            ..Default::default()
        };

        for record in &pending {
            let publish_result = self
                .publisher
                .publish(&record.subject, record.payload.clone())
                .await;
            let succeeded = publish_result.is_ok();
            let transition =
                next_after_attempt(record.attempts, self.policy.max_attempts, succeeded);
            let last_error = publish_result.as_ref().err().map(|e| e.to_string());

            self.store
                .apply_transition(&mut *tx, record.id, transition, last_error.as_deref())
                .await?;

            classify_pass(&mut report, succeeded, transition.status);
            if let Err(err) = publish_result {
                tracing::warn!(
                    outbox_id = %record.id,
                    subject = %record.subject,
                    attempts = transition.attempts,
                    error = %err,
                    "outbox publish attempt failed",
                );
            }
        }

        tx.commit().await?;
        Ok(report)
    }
}

/// Update the running [`RelayReport`] for one row's outcome.
fn classify_pass(report: &mut RelayReport, succeeded: bool, status: crate::outbox::OutboxStatus) {
    use crate::outbox::OutboxStatus::{Failed, Published};
    if succeeded {
        report.published += 1;
    } else if status == Failed {
        report.failed += 1;
    } else {
        report.retried += 1;
    }
    // `Published` only ever appears on success; the explicit import keeps the
    // match exhaustive-by-name without a wildcard that could hide a new state.
    let _ = Published;
}

/// Verify a durable consumer exists on `stream` before publishing — the honest
/// form of the medisup seed's `check_consumer`: a fail-fast for a critical
/// command whose receiver must be online (e.g. no worker is bound, so the
/// command would sit unconsumed). Returns
/// [`ConsumeErrorKind::NoConsumer`](crate::ConsumeErrorKind::NoConsumer) when the
/// consumer is absent, classified through the same layer the consumer shapes use.
///
/// This is **opt-in and separate from the relay**: it never auto-provisions, and
/// most events do not need it (a fact is published whether or not a subscriber
/// is currently online). Call it from the staging path when, and only when, the
/// receiver's presence is a precondition for issuing the command.
pub async fn verify_consumer(
    jetstream: &async_nats::jetstream::Context,
    stream: &str,
    consumer: &str,
) -> Result<(), IntegrationError> {
    let stream_handle = jetstream
        .get_stream(stream)
        .await
        .map_err(|e| IntegrationError::consume(ConsumeErrorKind::NoStream, e.to_string()))?;
    stream_handle
        .consumer_info(consumer)
        .await
        .map_err(|e| IntegrationError::consume(ConsumeErrorKind::NoConsumer, e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // GIVEN the default policy WHEN inspected THEN it carries the documented caps
    #[test]
    fn default_policy_has_documented_caps() {
        let p = RelayPolicy::default();
        assert_eq!(p.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(p.batch_size, DEFAULT_BATCH_SIZE);
    }

    // GIVEN a successful publish WHEN the pass is classified THEN it counts as published
    #[test]
    fn classify_counts_a_success_as_published() {
        let mut report = RelayReport::default();
        classify_pass(&mut report, true, crate::outbox::OutboxStatus::Published);
        assert_eq!(report.published, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(report.retried, 0);
    }

    // GIVEN a failure with retries left WHEN classified THEN it counts as retried
    #[test]
    fn classify_counts_a_retry() {
        let mut report = RelayReport::default();
        classify_pass(&mut report, false, crate::outbox::OutboxStatus::Pending);
        assert_eq!(report.retried, 1);
        assert_eq!(report.failed, 0);
    }

    // GIVEN a failure at the cap WHEN classified THEN it counts as failed
    #[test]
    fn classify_counts_a_terminal_failure() {
        let mut report = RelayReport::default();
        classify_pass(&mut report, false, crate::outbox::OutboxStatus::Failed);
        assert_eq!(report.failed, 1);
        assert_eq!(report.retried, 0);
    }
}
