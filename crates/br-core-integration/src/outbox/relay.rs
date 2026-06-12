//! The outbox **relay**: the subscribe-driven publisher that drains `Pending`
//! rows and persists each transition. Feature-gated behind `outbox`.
//!
//! The relay is the half of the outbox that makes "losing a critical
//! integration event is impossible" true. `stage` only guarantees the row is
//! durable *with* the domain write; the relay guarantees it is *published* — and
//! because it reads from the table rather than from an in-memory hand-off, the
//! exact same code that publishes right after a commit also re-publishes a row a
//! crash left behind (it was committed `Pending`, never published). There is no
//! separate recovery path to forget to run.
//!
//! ## Entry point: [`run`](OutboxRelay::run), not a timer
//!
//! [`run`](OutboxRelay::run) owns the loop: it does **one** startup recovery
//! drain, then parks on a `tokio::select!` that wakes on a Postgres
//! `LISTEN`/`NOTIFY` (fired by `stage` at commit), on a listener reconnect, or on
//! a chained retry deadline — never on a blind clock. When the outbox is clean
//! and no retry is owed it is genuinely parked at zero CPU and issues zero DB
//! traffic. The loop lives in [`driver`](super::driver); this file is the drain
//! itself. [`run_once`](OutboxRelay::run_once) is the building block it calls — a
//! single drain-until-empty pass (it processes **known** work and stops the
//! moment `fetch_one_pending` returns `None`, which is draining, not polling). It
//! stays `pub` for tests and manual operator recovery.
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
//! ## Structural vs transient failures
//!
//! A publish that fails because the target JetStream stream is **not declared**
//! ([`FailureClass::Structural`](super::retry::FailureClass::Structural)) is an
//! infra fault, not a delivery attempt: the row stays `Pending` and **does not
//! consume an attempt** against [`RelayPolicy::max_attempts`], so a
//! misconfiguration never marches a row to `Failed`. The relay flips its health
//! to [`Degraded`](super::RelayHealth::Degraded) for the consuming service's
//! readiness gate. A **transient** failure (timeout, broker blip) counts an
//! attempt and stays `Pending` until the cap (`Failed`), and drives the chained
//! retry deadline (see [`driver`](super::driver)).
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
use crate::outbox::health::{RelayHealthChannel, RelayHealthReceiver};
use crate::outbox::report::{RelayPolicy, RelayReport, classify_pass};
use crate::outbox::retry::{FailureClass, classify_failure};
use crate::outbox::status::{OutboxStatus, Transition, next_after_attempt};
use crate::outbox::store::{OutboxStore, OutboxStoreError};

/// Drains the outbox: reads `Pending` rows, publishes each through the shared
/// [`IntegrationPublisher`], and persists the transition the pure state machine
/// decides.
///
/// Holds a `pool` (its own connection source — the relay runs *after* the
/// caller's transaction, on its own transactions) and an `Arc<dyn
/// IntegrationPublisher>` (the same publisher the rest of the service uses, so a
/// `Noop` publisher makes the relay a no-op in tests).
pub struct OutboxRelay {
    pub(super) pool: sqlx::PgPool,
    pub(super) store: OutboxStore,
    publisher: Arc<dyn IntegrationPublisher>,
    pub(super) policy: RelayPolicy,
    pub(super) health: RelayHealthChannel,
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
            health: RelayHealthChannel::new(),
        }
    }

    /// A read handle on the relay's health, for the consuming service to bridge
    /// into its readiness gate. Starts [`Healthy`](crate::outbox::RelayHealth::Healthy);
    /// a structural publish failure (the target stream is not declared) flips it
    /// to [`Degraded`](crate::outbox::RelayHealth::Degraded) and a later
    /// structural-free pass restores it. Read it **before** spawning
    /// [`run`](Self::run) so the readiness wiring is in place from the start.
    ///
    /// `br-core-integration` is a `core` crate and must not depend on
    /// `br-util-axum-readiness` (a `util` crate); it exposes the raw signal and
    /// the service maps `Degraded` to a 503 — the wiring is shown as a seam in
    /// the crate README.
    pub fn health(&self) -> RelayHealthReceiver {
        self.health.receiver()
    }

    /// Run one drain pass: process `Pending` rows **one at a time**, each in its
    /// own short transaction (`BEGIN; SELECT … WHERE id > cursor FOR UPDATE SKIP
    /// LOCKED LIMIT 1; publish; apply_transition; COMMIT`), looping until no
    /// `Pending` row remains or [`RelayPolicy::max_messages`] rows have been
    /// processed. Returns a [`RelayReport`].
    ///
    /// This is a **single drain pass**, not a poll: it processes known work and
    /// returns the moment `fetch_one_pending` finds no more rows. The scheduling
    /// — *when* to drain — belongs to [`run`](Self::run); call `run_once`
    /// directly only for a test or a manual operator recovery sweep.
    ///
    /// The publish IO is never held inside a batch-locking transaction: a slow
    /// broker pins only the one row being published (and its connection), and a
    /// DB error on one row's transition rolls back **that row only** — never
    /// dozens of already-acked siblings. `FOR UPDATE SKIP LOCKED` keeps replicas
    /// draining disjoint rows.
    ///
    /// The pass advances an `id` cursor so each row is attempted **at most once
    /// per pass**: a row whose publish fails stays `Pending` but is not re-picked
    /// until the next wake, rather than the pass spinning on a persistently
    /// failing row. A **structural** failure (undeclared stream) leaves the row
    /// `Pending` without consuming an attempt and is reported in
    /// [`RelayReport::structural`].
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

        // A structural failure (undeclared stream) is NOT a delivery attempt:
        // keep the row `Pending` with its attempt count unchanged, record the
        // error, and report it so the loop flips health to `Degraded`. Anything
        // else (success or a transient failure) runs the normal attempt machine.
        let structural =
            publish_result.as_ref().err().map(classify_failure) == Some(FailureClass::Structural);

        let transition = if structural {
            // A structural fault is not an attempt: re-assert `Pending` with the
            // row's existing attempt count untouched, so the budget is preserved.
            Transition {
                status: OutboxStatus::Pending,
                attempts: record.attempts,
            }
        } else {
            next_after_attempt(
                record.attempts,
                self.policy.max_attempts,
                publish_result.is_ok(),
            )
        };
        let last_error = publish_result.as_ref().err().map(|e| e.to_string());

        self.store
            .apply_transition(&mut *tx, record.id, transition, last_error.as_deref())
            .await?;
        tx.commit().await?;

        report.picked += 1;
        classify_pass(report, &publish_result, transition, structural);
        if let Err(err) = publish_result {
            tracing::warn!(
                outbox_id = %record.id,
                subject = %record.subject,
                attempts = transition.attempts,
                structural,
                error = %err,
                "outbox publish attempt failed",
            );
        }
        Ok(Some(record.id))
    }
}
