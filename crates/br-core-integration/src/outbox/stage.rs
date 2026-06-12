//! The outbox **write side**: the same-transaction `stage` insert that fires the
//! notify-after-commit wake. Feature-gated behind `outbox`.
//!
//! Separated from `store.rs` (the relay's read/transition queries) so each file
//! is one responsibility: this is the only write a *caller* makes against the
//! outbox; the relay owns every subsequent transition.

use sqlx::{Executor, Postgres};

use crate::outbox::OutboxRecord;
use crate::outbox::store::{DEFAULT_TABLE, OutboxStoreError, notify_channel_for};
use crate::outbox::table_name::validate_table;

/// Stage `record` into the default `integration_outbox` table using the caller's
/// executor — pass `&mut *tx` so the insert lands in the **same transaction** as
/// the domain write. Idempotent on the row id (`ON CONFLICT (id) DO NOTHING`):
/// a retried request that re-stages the same UUIDv7 does not duplicate the row.
///
/// Within the same transaction it also fires `pg_notify` on the default table's
/// channel (see [`stage_into`]), so a [`run`](crate::OutboxRelay::run)-driven
/// relay is woken **at commit** rather than on a blind timer.
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
///
/// ## Notify-after-commit (wakes the relay, never on rollback)
///
/// In the **same transaction** it fires `pg_notify(<channel>, '')` on the
/// table's [notify channel](crate::OutboxStore::notify_channel). Postgres
/// delivers a `NOTIFY` issued inside a transaction **only when that transaction
/// commits**, and **never** on rollback — so the relay is woken exactly when a
/// row is durably committed, never for a write that rolled back (the same
/// notify-after-commit guarantee `br-util-broadcast` relies on). The channel is
/// passed as a bound *value* to `pg_notify` (not interpolated), so it carries no
/// injection surface; the empty payload keeps the wake a pure signal — the relay
/// reads the table for the actual work.
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
        "WITH inserted AS ( \
            INSERT INTO {table} (id, subject, payload, status, attempts) \
            VALUES ($1, $2, $3, $4, $5) \
            ON CONFLICT (id) DO NOTHING \
            RETURNING 1 \
         ) \
         SELECT pg_notify($6, '')"
    );
    sqlx::query(&sql)
        .bind(record.id)
        .bind(&record.subject)
        .bind(&record.payload)
        .bind(record.status.as_db_str())
        .bind(i64::from(record.attempts))
        .bind(notify_channel_for(table))
        .execute(executor)
        .await?;
    Ok(())
}
