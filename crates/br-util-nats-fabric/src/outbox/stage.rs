use sqlx::{Executor, Postgres};

use crate::outbox::record::OutboxRecord;
use crate::outbox::store::{OUTBOX_NOTIFY_CHANNEL, OutboxStoreError};

pub async fn stage<'e, E>(executor: E, record: &OutboxRecord) -> Result<(), OutboxStoreError>
where
    E: Executor<'e, Database = Postgres>,
{
    sqlx::query(
        "WITH inserted AS ( \
            INSERT INTO integration_outbox (id, subject, payload, status, attempts) \
            VALUES ($1, $2, $3, $4, $5) \
            ON CONFLICT (id) DO NOTHING \
            RETURNING 1 \
         ) \
         SELECT pg_notify($6, '')",
    )
    .bind(record.id)
    .bind(record.subject())
    .bind(&record.payload)
    .bind(record.status.as_db_str())
    .bind(i64::from(record.attempts))
    .bind(OUTBOX_NOTIFY_CHANNEL)
    .execute(executor)
    .await?;
    Ok(())
}
