use sqlx::{Executor, Postgres};
use uuid::Uuid;

use br_core_integration::{OutboxStatus, Transition};

use crate::coords::{EventCoords, EventSubjectParseError, parse_event_subject};

pub const OUTBOX_TABLE: &str = "integration_outbox";
pub const OUTBOX_NOTIFY_CHANNEL: &str = "integration_outbox";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingOutbox {
    pub id: Uuid,
    pub destination: EventCoords,
    pub payload: serde_json::Value,
    pub attempts: u32,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OutboxStore;

impl OutboxStore {
    pub fn new() -> Self {
        Self
    }

    pub async fn fetch_one_pending<'e, E>(
        &self,
        executor: E,
        after: Uuid,
    ) -> Result<Option<PendingOutbox>, OutboxStoreError>
    where
        E: Executor<'e, Database = Postgres>,
    {
        let row: Option<OutboxRow> = sqlx::query_as(
            "SELECT id, subject, payload, attempts \
             FROM integration_outbox \
             WHERE status = 'PENDING' AND id > $1 \
             ORDER BY id \
             LIMIT 1 \
             FOR UPDATE SKIP LOCKED",
        )
        .bind(after)
        .fetch_optional(executor)
        .await?;
        row.map(OutboxRow::into_pending).transpose()
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
            "UPDATE integration_outbox \
             SET status = $2, attempts = $3, last_error = $4, {published_at_clause} \
             WHERE id = $1"
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
    #[error("outbox query failed: {0}")]
    Sql(#[from] sqlx::Error),
    #[error("outbox row {id} has an unrenderable destination: {source}")]
    Destination {
        id: Uuid,
        #[source]
        source: EventSubjectParseError,
    },
}

#[derive(sqlx::FromRow)]
struct OutboxRow {
    id: Uuid,
    subject: String,
    payload: serde_json::Value,
    attempts: i64,
}

impl OutboxRow {
    fn into_pending(self) -> Result<PendingOutbox, OutboxStoreError> {
        let destination =
            parse_event_subject(&self.subject).map_err(|source| OutboxStoreError::Destination {
                id: self.id,
                source,
            })?;
        Ok(PendingOutbox {
            id: self.id,
            destination,
            payload: self.payload,
            attempts: self.attempts.max(0) as u32,
        })
    }
}
