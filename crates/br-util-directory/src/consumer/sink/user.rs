use std::collections::BTreeSet;
use std::sync::LazyLock;

use br_core_directory::{PublishedUser, user_id_from_kv_key, user_kv_key};
use br_util_nats_fabric::{KvKey, ProjectionSink};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::consumer::config::DirectoryConsumerConfig;
use crate::consumer::sink::context::SinkContext;
use crate::consumer::sink::upsert::change_detecting_upsert;
use crate::error::DirectoryError;
use crate::impact::{ForeignRef, Impact};

const COLUMNS: [&str; 4] = ["email", "first_name", "last_name", "extensions"];

static UPSERT: LazyLock<String> =
    LazyLock::new(|| change_detecting_upsert("known_users", "user_id", &COLUMNS));

const MEMBERSHIPS: &str = "SELECT group_id FROM known_user_group WHERE user_id = $1";

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

        self.context
            .apply_single_statement(async |conn| {
                let changed = sqlx::query(UPSERT.as_str())
                    .bind(user_id)
                    .bind(&value.email)
                    .bind(&value.first_name)
                    .bind(&value.last_name)
                    .bind(extensions)
                    .execute(&mut *conn)
                    .await?
                    .rows_affected()
                    > 0;
                Ok(match changed {
                    true => vec![Impact::changed(ForeignRef::user(user_id))],
                    false => Vec::new(),
                })
            })
            .await
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(user_id) = user_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        self.context
            .apply_single_statement(async |conn| {
                let deleted = sqlx::query("DELETE FROM known_users WHERE user_id = $1")
                    .bind(user_id)
                    .execute(&mut *conn)
                    .await?
                    .rows_affected()
                    > 0;
                if !deleted {
                    return Ok(Vec::new());
                }
                let mut impacts = vec![Impact::changed(ForeignRef::user(user_id))];
                if self.context.stages_impacts() {
                    for group_id in memberships_of(conn, user_id).await? {
                        impacts.push(Impact::changed(ForeignRef::group(group_id)));
                    }
                }
                Ok(impacts)
            })
            .await
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

async fn memberships_of(
    conn: &mut PgConnection,
    user_id: Uuid,
) -> Result<Vec<Uuid>, DirectoryError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(MEMBERSHIPS)
        .bind(user_id)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().map(|(group_id,)| group_id).collect())
}
