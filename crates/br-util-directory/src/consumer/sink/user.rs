use std::collections::BTreeSet;

use br_core_directory::{PublishedUser, user_id_from_kv_key, user_kv_key};
use br_util_nats_fabric::{KvKey, ProjectionSink};

use crate::consumer::config::DirectoryConsumerConfig;
use crate::consumer::sink::context::SinkContext;
use crate::error::DirectoryError;
use crate::impact::ForeignRef;

const UPSERT: &str = "INSERT INTO known_users AS t \
     (user_id, email, first_name, last_name, extensions) \
     VALUES ($1, $2, $3, $4, $5) \
     ON CONFLICT (user_id) DO UPDATE \
     SET email = EXCLUDED.email, \
         first_name = EXCLUDED.first_name, \
         last_name = EXCLUDED.last_name, \
         extensions = EXCLUDED.extensions \
     WHERE (t.email, t.first_name, t.last_name, t.extensions) \
        IS DISTINCT FROM \
           (EXCLUDED.email, EXCLUDED.first_name, EXCLUDED.last_name, EXCLUDED.extensions)";

pub(crate) struct UserSink {
    context: SinkContext,
    config: DirectoryConsumerConfig,
}

impl UserSink {
    pub(crate) fn new(context: SinkContext, config: DirectoryConsumerConfig) -> Self {
        Self { context, config }
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

        let mut tx = self.context.pool().begin().await?;
        let changed = sqlx::query(UPSERT)
            .bind(user_id)
            .bind(&value.email)
            .bind(&value.first_name)
            .bind(&value.last_name)
            .bind(extensions)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        if changed {
            self.context
                .stage_change(&mut tx, || ForeignRef::user(user_id))
                .await?;
        }
        tx.commit().await?;

        if changed {
            self.context.record_change();
        }
        Ok(())
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(user_id) = user_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        let mut tx = self.context.pool().begin().await?;
        let deleted = sqlx::query("DELETE FROM known_users WHERE user_id = $1")
            .bind(user_id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        if deleted {
            self.context
                .stage_change(&mut tx, || ForeignRef::user(user_id))
                .await?;
        }
        tx.commit().await?;

        if deleted {
            self.context.record_change();
        }
        Ok(())
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        let ids: Vec<(uuid::Uuid,)> = sqlx::query_as("SELECT user_id FROM known_users")
            .fetch_all(self.context.pool())
            .await?;
        ids.into_iter()
            .map(|(id,)| KvKey::new(user_kv_key(id)).map_err(DirectoryError::from))
            .collect()
    }
}
