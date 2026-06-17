use std::collections::BTreeSet;

use br_core_directory::{PublishedGroup, group_id_from_kv_key, group_kv_key};
use br_util_nats_fabric::{KvKey, ProjectionSink};
use sqlx::PgPool;

use crate::consumer::recompose::member_rows;
use crate::error::DirectoryError;

pub(crate) struct GroupSink {
    pool: PgPool,
}

impl GroupSink {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait::async_trait]
impl ProjectionSink<PublishedGroup> for GroupSink {
    type Error = DirectoryError;

    async fn project(&self, key: &KvKey, value: &PublishedGroup) -> Result<(), Self::Error> {
        let Some(group_id) = group_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            "INSERT INTO known_groups (group_id, name) VALUES ($1, $2) \
             ON CONFLICT (group_id) DO UPDATE SET name = EXCLUDED.name",
        )
        .bind(group_id)
        .bind(&value.name)
        .execute(&mut *tx)
        .await?;

        sqlx::query("DELETE FROM known_user_group WHERE group_id = $1")
            .bind(group_id)
            .execute(&mut *tx)
            .await?;

        for row in member_rows(group_id, value) {
            sqlx::query(
                "INSERT INTO known_user_group (group_id, user_id) VALUES ($1, $2) \
                 ON CONFLICT (group_id, user_id) DO NOTHING",
            )
            .bind(row.group_id)
            .bind(row.user_id)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(group_id) = group_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };
        sqlx::query("DELETE FROM known_groups WHERE group_id = $1")
            .bind(group_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        let ids: Vec<(uuid::Uuid,)> = sqlx::query_as("SELECT group_id FROM known_groups")
            .fetch_all(&self.pool)
            .await?;
        ids.into_iter()
            .map(|(id,)| KvKey::new(group_kv_key(id)).map_err(DirectoryError::from))
            .collect()
    }
}
