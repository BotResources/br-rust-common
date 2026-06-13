use sqlx::{Executor, Postgres};
use uuid::Uuid;

use crate::outbox::OutboxRecord;
use crate::outbox::status::Transition;
use crate::outbox::{OutboxStatus, table_name::validate_table};

pub const DEFAULT_TABLE: &str = "integration_outbox";

pub(super) fn notify_channel_for(table: &str) -> String {
    table.to_string()
}

#[derive(Debug, Clone)]
pub struct OutboxStore {
    table: String,
}

impl Default for OutboxStore {
    fn default() -> Self {
        Self::new(DEFAULT_TABLE).expect("DEFAULT_TABLE is a valid outbox identifier")
    }
}

impl OutboxStore {
    pub fn new(table: impl Into<String>) -> Result<Self, OutboxStoreError> {
        let table = table.into();
        validate_table(&table)?;
        Ok(Self { table })
    }

    pub fn table(&self) -> &str {
        &self.table
    }

    pub fn notify_channel(&self) -> String {
        notify_channel_for(&self.table)
    }

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

    pub async fn apply_transition<'e, E>(
        &self,
        executor: E,
        id: Uuid,
        transition: Transition,
        last_error: Option<&str>,
    ) -> Result<(), OutboxStoreError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let published_at_clause = if transition.status == OutboxStatus::Published {
            "published_at = NOW()"
        } else {
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

#[derive(thiserror::Error, Debug)]
#[non_exhaustive]
pub enum OutboxStoreError {
    #[error("invalid outbox table name: {table:?}")]
    InvalidTable { table: String },
    #[error("outbox query failed: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("outbox row {id} has an unknown status: {value:?}")]
    UnknownStatus { id: Uuid, value: String },
}

#[derive(sqlx::FromRow)]
struct OutboxRow {
    id: Uuid,
    subject: String,
    payload: serde_json::Value,
    status: String,
    attempts: i64,
}

impl OutboxRow {
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

    #[test]
    fn new_accepts_a_valid_table() {
        assert_eq!(
            OutboxStore::new("svc_chat_outbox").unwrap().table(),
            "svc_chat_outbox"
        );
    }

    #[test]
    fn new_rejects_an_unsafe_table() {
        let err = OutboxStore::new("outbox;DROP TABLE users").unwrap_err();
        assert!(matches!(err, OutboxStoreError::InvalidTable { .. }));
    }

    #[test]
    fn default_store_uses_the_canonical_table() {
        assert_eq!(OutboxStore::default().table(), DEFAULT_TABLE);
    }

    #[test]
    fn notify_channel_is_the_table_name() {
        assert_eq!(OutboxStore::default().notify_channel(), DEFAULT_TABLE);
        assert_eq!(
            OutboxStore::new("svc_chat_outbox")
                .unwrap()
                .notify_channel(),
            "svc_chat_outbox"
        );
    }
}
