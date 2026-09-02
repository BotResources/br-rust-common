use std::collections::BTreeSet;

use br_core_directory::{PublishedGroup, group_id_from_kv_key, group_kv_key};
use br_util_nats_fabric::{KvKey, ProjectionSink};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::consumer::recompose::member_rows;
use crate::consumer::sink::context::SinkContext;
use crate::error::DirectoryError;
use crate::impact::ForeignRef;

const UPSERT: &str = "INSERT INTO known_groups AS t (group_id, name) VALUES ($1, $2) \
     ON CONFLICT (group_id) DO UPDATE SET name = EXCLUDED.name \
     WHERE t.name IS DISTINCT FROM EXCLUDED.name";

const LOCKED_MEMBERS: &str =
    "SELECT user_id FROM known_user_group WHERE group_id = $1 ORDER BY user_id FOR UPDATE";

pub(crate) struct GroupSink {
    context: SinkContext,
}

impl GroupSink {
    pub(crate) fn new(context: SinkContext) -> Self {
        Self { context }
    }
}

#[async_trait::async_trait]
impl ProjectionSink<PublishedGroup> for GroupSink {
    type Error = DirectoryError;

    async fn project(&self, key: &KvKey, value: &PublishedGroup) -> Result<(), Self::Error> {
        let Some(group_id) = group_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        let mut tx = self.context.pool().begin().await?;

        let name_changed = sqlx::query(UPSERT)
            .bind(group_id)
            .bind(&value.name)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;

        let projected: BTreeSet<Uuid> = member_rows(group_id, value)
            .into_iter()
            .map(|row| row.user_id)
            .collect();
        let members_changed = locked_members(&mut tx, group_id).await? != projected;
        if members_changed {
            rewrite_members(&mut tx, group_id, &projected).await?;
        }

        let changed = name_changed || members_changed;
        if changed {
            self.context
                .stage_change(&mut tx, || ForeignRef::group(group_id))
                .await?;
        }
        tx.commit().await?;

        if changed {
            self.context.record_change();
        }
        Ok(())
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(group_id) = group_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        let mut tx = self.context.pool().begin().await?;
        let deleted = sqlx::query("DELETE FROM known_groups WHERE group_id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        if deleted {
            self.context
                .stage_change(&mut tx, || ForeignRef::group(group_id))
                .await?;
        }
        tx.commit().await?;

        if deleted {
            self.context.record_change();
        }
        Ok(())
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        let ids: Vec<(uuid::Uuid,)> = sqlx::query_as("SELECT group_id FROM known_groups")
            .fetch_all(self.context.pool())
            .await?;
        ids.into_iter()
            .map(|(id,)| KvKey::new(group_kv_key(id)).map_err(DirectoryError::from))
            .collect()
    }
}

async fn locked_members(
    conn: &mut PgConnection,
    group_id: Uuid,
) -> Result<BTreeSet<Uuid>, DirectoryError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(LOCKED_MEMBERS)
        .bind(group_id)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().map(|(user_id,)| user_id).collect())
}

async fn rewrite_members(
    conn: &mut PgConnection,
    group_id: Uuid,
    members: &BTreeSet<Uuid>,
) -> Result<(), DirectoryError> {
    sqlx::query("DELETE FROM known_user_group WHERE group_id = $1")
        .bind(group_id)
        .execute(&mut *conn)
        .await?;

    sqlx::query(
        "INSERT INTO known_user_group (group_id, user_id) \
         SELECT $1, unnest($2::uuid[]) \
         ON CONFLICT (group_id, user_id) DO NOTHING",
    )
    .bind(group_id)
    .bind(members.iter().copied().collect::<Vec<Uuid>>())
    .execute(conn)
    .await?;
    Ok(())
}
