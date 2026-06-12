//! The Postgres outbox store: the same-transaction `stage` insert and the
//! relay's read/transition queries. Feature-gated behind `outbox` (the only
//! sqlx-touching surface of this crate).
//!
//! The store never auto-provisions: the `integration_outbox` table is a
//! **declared object** the consuming service's migrations own (the canonical
//! DDL is in the crate README). A missing table fails loud as a sqlx error.

use sqlx::{Executor, Postgres};
use uuid::Uuid;

use crate::outbox::status::Transition;
use crate::outbox::table_name::validate_table;
use crate::outbox::{OutboxRecord, OutboxStatus};

/// The default outbox table name. Override with [`stage_into`] /
/// [`OutboxStore::new`] for a service that names it differently.
pub const DEFAULT_TABLE: &str = "integration_outbox";

/// Stage `record` into the default `integration_outbox` table using the caller's
/// executor — pass `&mut *tx` so the insert lands in the **same transaction** as
/// the domain write. Idempotent on the row id (`ON CONFLICT (id) DO NOTHING`):
/// a retried request that re-stages the same UUIDv7 does not duplicate the row.
///
/// This is the only write the caller makes against the outbox; the relay owns
/// every subsequent transition.
pub async fn stage<'e, E>(executor: E, record: &OutboxRecord) -> Result<(), OutboxStoreError>
where
    E: Executor<'e, Database = Postgres>,
{
    stage_into(executor, DEFAULT_TABLE, record).await
}

/// Stage `record` into an explicitly named table. See [`stage`]. The table name
/// is interpolated into the SQL (PG cannot bind an identifier), so it is
/// validated structurally first and rejected as a typed
/// [`OutboxStoreError::InvalidTable`] if it is not a plain `^[a-z_][a-z0-9_]*$`
/// identifier — never a place to pass user input.
pub async fn stage_into<'e, E>(
    executor: E,
    table: &str,
    record: &OutboxRecord,
) -> Result<(), OutboxStoreError>
where
    E: Executor<'e, Database = Postgres>,
{
    validate_table(table)?;
    let sql = format!(
        "INSERT INTO {table} (id, subject, payload, status, attempts) \
         VALUES ($1, $2, $3, $4, $5) \
         ON CONFLICT (id) DO NOTHING"
    );
    sqlx::query(&sql)
        .bind(record.id)
        .bind(&record.subject)
        .bind(&record.payload)
        .bind(record.status.as_db_str())
        .bind(i64::from(record.attempts))
        .execute(executor)
        .await?;
    Ok(())
}

/// A handle over a named outbox table that the [`OutboxRelay`](crate::OutboxRelay)
/// uses to read pending rows and persist transitions. Holds no connection — each
/// method takes the caller's executor, so the relay controls transaction scope.
#[derive(Debug, Clone)]
pub struct OutboxStore {
    table: String,
}

impl Default for OutboxStore {
    fn default() -> Self {
        // DEFAULT_TABLE is a known-valid identifier; the validation cannot fail.
        Self::new(DEFAULT_TABLE).expect("DEFAULT_TABLE is a valid outbox identifier")
    }
}

impl OutboxStore {
    /// A store over `table`. The name is interpolated into SQL (PG cannot bind
    /// an identifier), so it is validated structurally at construction:
    /// a name that is not a plain `^[a-z_][a-z0-9_]*$` identifier is rejected as
    /// [`OutboxStoreError::InvalidTable`] rather than trusted by comment.
    pub fn new(table: impl Into<String>) -> Result<Self, OutboxStoreError> {
        let table = table.into();
        validate_table(&table)?;
        Ok(Self { table })
    }

    /// The table this store reads and writes.
    pub fn table(&self) -> &str {
        &self.table
    }

    /// Fetch and lock the single oldest `Pending` row with `id > after`
    /// (`ORDER BY id LIMIT 1`, UUIDv7 ids sort chronologically), or `None` if no
    /// such row remains. `FOR UPDATE SKIP LOCKED` lets concurrent relay replicas
    /// each pick a *different* row — none is processed twice.
    ///
    /// `after` is the relay's per-pass cursor: pass [`Uuid::nil`] to start from
    /// the oldest row, then the last-processed id to advance. It guarantees the
    /// relay attempts each row **at most once per pass** — a row that fails stays
    /// `Pending` but, because the cursor has moved past its id, is not re-picked
    /// until the next pass (the caller's interval is the retry backoff), instead
    /// of spinning on the same failing row within one pass.
    ///
    /// Pass a transaction executor (`&mut *tx`): the row stays locked until the
    /// relay commits its transition, so a crash mid-publish releases the lock and
    /// a later pass re-picks the still-`Pending` row. The lock is held for only
    /// one publish.
    pub async fn fetch_one_pending<'e, E>(
        &self,
        executor: E,
        after: Uuid,
    ) -> Result<Option<OutboxRecord>, OutboxStoreError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            "SELECT id, subject, payload, status, attempts \
             FROM {} \
             WHERE status = 'PENDING' AND id > $1 \
             ORDER BY id \
             LIMIT 1 \
             FOR UPDATE SKIP LOCKED",
            self.table
        );
        let row: Option<OutboxRow> = sqlx::query_as(&sql)
            .bind(after)
            .fetch_optional(executor)
            .await?;
        row.map(OutboxRow::into_record).transpose()
    }

    /// Fetch up to `limit` `Pending` rows, oldest first (UUIDv7 ids sort by
    /// creation time, so ordering by `id` is chronological). `FOR UPDATE SKIP
    /// LOCKED` lets multiple relay replicas drain the outbox concurrently without
    /// double-publishing a row. Used by tests/diagnostics to inspect the queue;
    /// the relay itself processes one row per transaction via
    /// [`fetch_one_pending`](Self::fetch_one_pending).
    ///
    /// Pass a transaction executor (`&mut *tx`): the rows stay locked until the
    /// caller commits, so a crash releases the lock and another pass re-picks the
    /// still-`Pending` rows.
    pub async fn fetch_pending<'e, E>(
        &self,
        executor: E,
        limit: i64,
    ) -> Result<Vec<OutboxRecord>, OutboxStoreError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let sql = format!(
            "SELECT id, subject, payload, status, attempts \
             FROM {} \
             WHERE status = 'PENDING' \
             ORDER BY id \
             LIMIT $1 \
             FOR UPDATE SKIP LOCKED",
            self.table
        );
        let rows: Vec<OutboxRow> = sqlx::query_as(&sql).bind(limit).fetch_all(executor).await?;
        rows.into_iter().map(OutboxRow::into_record).collect()
    }

    /// Persist a relay [`Transition`] for one row: write the new status and
    /// attempt count, and — on success — stamp `published_at`. On the terminal
    /// `Failed` path, record `last_error` for diagnosis.
    ///
    /// `last_error` is written unconditionally (to the bound value or `NULL`), so
    /// a previously-failed row that finally publishes has its `last_error`
    /// **reset to NULL** — the column always reflects the *latest* attempt, never
    /// a stale earlier failure. The relay passes `None` on a successful publish.
    pub async fn apply_transition<'e, E>(
        &self,
        executor: E,
        id: Uuid,
        transition: Transition,
        last_error: Option<&str>,
    ) -> Result<(), sqlx::Error>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let published_at_clause = if transition.status == OutboxStatus::Published {
            "published_at = NOW()"
        } else {
            // Intentional self-assignment, not a bug: a non-publish transition
            // (still `Pending`, or terminal `Failed`) must leave `published_at`
            // untouched, and keeping one UPDATE shape (rather than branching the
            // SET list) keeps the query stable. PG optimizes the no-op away.
            "published_at = published_at"
        };
        let sql = format!(
            "UPDATE {} \
             SET status = $2, attempts = $3, last_error = $4, {published_at_clause} \
             WHERE id = $1",
            self.table
        );
        sqlx::query(&sql)
            .bind(id)
            .bind(transition.status.as_db_str())
            .bind(i64::from(transition.attempts))
            .bind(last_error)
            .execute(executor)
            .await?;
        Ok(())
    }
}

/// Why an outbox operation failed: an invalid table name (rejected before any
/// SQL), a transport/SQL error, or a row whose stored status string is not a
/// known [`OutboxStatus`] (a corrupt or future row — fail loud, never coerce it
/// into a default).
#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum OutboxStoreError {
    /// The table name is not a plain `^[a-z_][a-z0-9_]*$` identifier. Rejected at
    /// construction / staging, before it is ever interpolated into SQL — PG
    /// cannot bind an identifier, so the name is validated structurally instead
    /// of trusted.
    #[error("invalid outbox table name: {table:?}")]
    InvalidTable { table: String },
    /// The underlying sqlx query failed (table missing, connection lost, …).
    #[error("outbox query failed: {0}")]
    Sql(#[from] sqlx::Error),
    /// A row's `status` column held a value outside the known set.
    #[error("outbox row {id} has an unknown status: {value:?}")]
    UnknownStatus { id: Uuid, value: String },
}

/// The raw row shape `fetch_pending` decodes before validating its status.
#[derive(sqlx::FromRow)]
struct OutboxRow {
    id: Uuid,
    subject: String,
    payload: serde_json::Value,
    status: String,
    attempts: i64,
}

impl OutboxRow {
    /// Hydrate into a typed [`OutboxRecord`], re-validating the status string
    /// (an unknown value is a fail-loud [`OutboxStoreError::UnknownStatus`], not
    /// a silent default) and clamping a negative attempt count to zero.
    fn into_record(self) -> Result<OutboxRecord, OutboxStoreError> {
        let status = OutboxStatus::from_db_str(&self.status).map_err(|e| {
            OutboxStoreError::UnknownStatus {
                id: self.id,
                value: e.0,
            }
        })?;
        Ok(OutboxRecord {
            id: self.id,
            subject: self.subject,
            payload: self.payload,
            status,
            attempts: self.attempts.max(0) as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GIVEN a valid name WHEN a store is built THEN it constructs and carries it
    #[test]
    fn new_accepts_a_valid_table() {
        assert_eq!(
            OutboxStore::new("svc_chat_outbox").unwrap().table(),
            "svc_chat_outbox"
        );
    }

    // GIVEN an unsafe name WHEN a store is built THEN it fails with InvalidTable
    // (the structural guard is exercised exhaustively in `table_name`'s tests).
    #[test]
    fn new_rejects_an_unsafe_table() {
        let err = OutboxStore::new("outbox;DROP TABLE users").unwrap_err();
        assert!(matches!(err, OutboxStoreError::InvalidTable { .. }));
    }

    // GIVEN the default store WHEN built THEN it carries the canonical table name
    #[test]
    fn default_store_uses_the_canonical_table() {
        assert_eq!(OutboxStore::default().table(), DEFAULT_TABLE);
    }
}
