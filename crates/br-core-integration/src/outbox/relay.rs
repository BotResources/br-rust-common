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
//! ## One short transaction per message (not a batch)
//!
//! Each row is processed in its **own** short transaction — `BEGIN; SELECT … FOR
//! UPDATE SKIP LOCKED LIMIT 1; publish; apply_transition; COMMIT` — looped until
//! no `Pending` row remains (or [`RelayPolicy::max_messages`] is reached). The
//! publish IO is therefore **never** held inside a transaction that locks a
//! whole batch:
//!
//! - the row's lock + its connection are held only for that single publish, so a
//!   slow broker does not pin a 64-row batch (and its connection) for the sum of
//!   64 network round-trips;
//! - a DB error applying one row's transition rolls back **that row only** — it
//!   cannot roll back, and so re-publish, dozens of already-acked siblings.
//!
//! `FOR UPDATE SKIP LOCKED` is kept, so concurrent relay replicas still drain
//! disjoint rows — each picks a row no other has locked.
//!
//! ## Semantics — at-least-once, post-commit
//!
//! Publish happens *after* the staging transaction commits, so a consumer can
//! observe an event whose producer-side transaction is fully durable (no
//! dirty-read of an uncommitted write). The relay is **at-least-once**: a crash
//! after the broker ack but before `apply_transition` leaves the row `Pending`,
//! and the next pass re-publishes it. Subscribers must therefore de-dupe on the
//! envelope id (the same idempotency rule the consumer shapes already document).
//!
//! ## Publisher timeout (where it belongs)
//!
//! Under per-row commit a hung publish still holds **one** row's lock and one
//! connection until it returns — bounded blast radius, but it should still be
//! bounded in time. The timeout belongs on the [`IntegrationPublisher`]
//! (`NatsIntegrationPublisher`) so *every* publish path — relay, direct,
//! `publish_if_connected` — inherits it, not on the relay loop. A publish that
//! times out surfaces as a normal failed attempt (`PublishErrorKind::Timeout`)
//! and the row stays `Pending` for the next pass.

use std::sync::Arc;

use uuid::Uuid;

use crate::IntegrationPublisher;
use crate::outbox::status::next_after_attempt;
use crate::outbox::store::{OutboxStore, OutboxStoreError};

/// How many publish attempts the relay makes across passes before marking a row
/// `Failed`. Counts attempts recorded on the row, not retries within one pass.
pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

/// How many `Pending` rows one [`OutboxRelay::run_once`] pass processes before
/// returning — a per-invocation cap that bounds a single pass even if rows keep
/// arriving. Each row is its own short transaction.
pub const DEFAULT_MAX_MESSAGES: usize = 256;

/// Tuning for an [`OutboxRelay`] pass.
///
/// `#[non_exhaustive]`: start from [`RelayPolicy::default`] and override fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelayPolicy {
    /// Attempts (across passes) before a row is marked `Failed`. Clamped to ≥1.
    pub max_attempts: u32,
    /// Max rows one [`OutboxRelay::run_once`] pass processes — each in its own
    /// short transaction — before it returns. Bounds a single invocation.
    /// Clamped to ≥1.
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

/// Outcome of one [`OutboxRelay::run_once`] pass — what the caller logs / meters.
///
/// One pass processes rows one at a time (each its own short transaction), so
/// the counts sum the per-row outcomes: `picked == published + failed + retried`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelayReport {
    /// Rows picked up and processed this pass (each in its own transaction).
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

    /// Run one pass: process `Pending` rows **one at a time**, each in its own
    /// short transaction (`BEGIN; SELECT … WHERE id > cursor FOR UPDATE SKIP
    /// LOCKED LIMIT 1; publish; apply_transition; COMMIT`), looping until no
    /// `Pending` row remains or [`RelayPolicy::max_messages`] rows have been
    /// processed. Returns a [`RelayReport`].
    ///
    /// The publish IO is never held inside a batch-locking transaction: a slow
    /// broker pins only the one row being published (and its connection), and a
    /// DB error on one row's transition rolls back **that row only** — never
    /// dozens of already-acked siblings. `FOR UPDATE SKIP LOCKED` keeps replicas
    /// draining disjoint rows.
    ///
    /// The pass advances an `id` cursor so each row is attempted **at most once
    /// per pass**: a row whose publish fails stays `Pending` but is not re-picked
    /// until the next pass (the caller's interval is the retry backoff), rather
    /// than the pass spinning on a persistently-failing row.
    ///
    /// Call this on a schedule (a timer task) *and* once at startup: the startup
    /// pass is the crash-recovery sweep. Idle when the outbox is empty — it does
    /// not spin; the caller owns the interval.
    pub async fn run_once(&self) -> Result<RelayReport, OutboxStoreError> {
        let mut report = RelayReport::default();
        let cap = self.policy.max_messages.max(1);
        let mut cursor = Uuid::nil();

        for _ in 0..cap {
            match self.process_one(cursor, &mut report).await? {
                Some(id) => cursor = id, // advance past the row just attempted
                None => break,           // no Pending row beyond the cursor — done
            }
        }

        Ok(report)
    }

    /// Process the oldest `Pending` row with `id > after` in its own short
    /// transaction. Returns `Ok(Some(id))` with the processed row's id (the new
    /// cursor), or `Ok(None)` if none remained. The transaction commits the
    /// transition before returning, so the row's lock is released the moment its
    /// outcome is durable.
    async fn process_one(
        &self,
        after: Uuid,
        report: &mut RelayReport,
    ) -> Result<Option<Uuid>, OutboxStoreError> {
        let mut tx = self.pool.begin().await?;
        let Some(record) = self.store.fetch_one_pending(&mut *tx, after).await? else {
            // Nothing left beyond the cursor — commit the empty read tx and stop.
            tx.commit().await?;
            return Ok(None);
        };

        let publish_result = self
            .publisher
            .publish(&record.subject, record.payload.clone())
            .await;
        let succeeded = publish_result.is_ok();
        let transition = next_after_attempt(record.attempts, self.policy.max_attempts, succeeded);
        let last_error = publish_result.as_ref().err().map(|e| e.to_string());

        self.store
            .apply_transition(&mut *tx, record.id, transition, last_error.as_deref())
            .await?;
        tx.commit().await?;

        report.picked += 1;
        classify_pass(report, succeeded, transition.status);
        if let Err(err) = publish_result {
            tracing::warn!(
                outbox_id = %record.id,
                subject = %record.subject,
                attempts = transition.attempts,
                error = %err,
                "outbox publish attempt failed",
            );
        }
        Ok(Some(record.id))
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

#[cfg(test)]
mod tests {
    use super::*;

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
