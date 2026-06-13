use sqlx::{Executor, Postgres};

use crate::outbox::OutboxRecord;
use crate::outbox::store::{DEFAULT_TABLE, OutboxStoreError, notify_channel_for};
use crate::outbox::table_name::validate_table;

pub async fn stage<'e, E>(executor: E, record: &OutboxRecord) -> Result<(), OutboxStoreError>
where
    E: Executor<'e, Database = Postgres>,
{
    stage_into(executor, DEFAULT_TABLE, record).await
}

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
