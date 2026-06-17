use std::collections::BTreeSet;

use br_core_directory::{PublishedUser, user_id_from_kv_key, user_kv_key};
use br_util_nats_fabric::{KvKey, ProjectionSink};
use sqlx::PgPool;

use crate::consumer::config::DirectoryConsumerConfig;
use crate::error::DirectoryError;

pub(crate) struct UserSink {
    pool: PgPool,
    config: DirectoryConsumerConfig,
}

impl UserSink {
    pub(crate) fn new(pool: PgPool, config: DirectoryConsumerConfig) -> Self {
        Self { pool, config }
    }
}

#[async_trait::async_trait]
impl ProjectionSink<PublishedUser> for UserSink {
    type Error = DirectoryError;

    async fn project(&self, key: &KvKey, value: &PublishedUser) -> Result<(), Self::Error> {
        let Some(user_id) = user_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };
        let extensions = self.config.extract_for(value).into_value();
        sqlx::query(
            "INSERT INTO known_users (user_id, email, first_name, last_name, extensions) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (user_id) DO UPDATE \
             SET email = EXCLUDED.email, \
                 first_name = EXCLUDED.first_name, \
                 last_name = EXCLUDED.last_name, \
                 extensions = EXCLUDED.extensions",
        )
        .bind(user_id)
        .bind(&value.email)
        .bind(&value.first_name)
        .bind(&value.last_name)
        .bind(extensions)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(user_id) = user_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };
        sqlx::query("DELETE FROM known_users WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        let ids: Vec<(uuid::Uuid,)> = sqlx::query_as("SELECT user_id FROM known_users")
            .fetch_all(&self.pool)
            .await?;
        ids.into_iter()
            .map(|(id,)| KvKey::new(user_kv_key(id)).map_err(DirectoryError::from))
            .collect()
    }
}
