use std::collections::BTreeSet;
use std::sync::LazyLock;

use br_core_directory::{
    PublishedServiceAccount, service_account_id_from_kv_key, service_account_kv_key,
};
use br_util_nats_fabric::{KvKey, ProjectionSink};

use crate::consumer::sink::context::SinkContext;
use crate::consumer::sink::upsert::change_detecting_upsert;
use crate::error::DirectoryError;
use crate::impact::{ForeignRef, Impact};

static UPSERT: LazyLock<String> = LazyLock::new(|| {
    change_detecting_upsert("known_service_accounts", "service_account_id", &["name"])
});

pub(crate) struct ServiceAccountSink {
    context: SinkContext,
}

impl ServiceAccountSink {
    pub(crate) fn new(context: SinkContext) -> Self {
        Self { context }
    }
}

#[async_trait::async_trait]
impl ProjectionSink<PublishedServiceAccount> for ServiceAccountSink {
    type Error = DirectoryError;

    async fn project(
        &self,
        key: &KvKey,
        value: &PublishedServiceAccount,
    ) -> Result<(), Self::Error> {
        let Some(service_account_id) = service_account_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        self.context
            .apply_single_statement(async |conn| {
                let changed = sqlx::query(UPSERT.as_str())
                    .bind(service_account_id)
                    .bind(&value.name)
                    .execute(&mut *conn)
                    .await?
                    .rows_affected()
                    > 0;
                Ok(match changed {
                    true => vec![Impact::changed(ForeignRef::service_account(
                        service_account_id,
                    ))],
                    false => Vec::new(),
                })
            })
            .await
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(service_account_id) = service_account_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        self.context
            .apply_single_statement(async |conn| {
                let deleted =
                    sqlx::query("DELETE FROM known_service_accounts WHERE service_account_id = $1")
                        .bind(service_account_id)
                        .execute(&mut *conn)
                        .await?
                        .rows_affected()
                        > 0;
                Ok(match deleted {
                    true => vec![Impact::changed(ForeignRef::service_account(
                        service_account_id,
                    ))],
                    false => Vec::new(),
                })
            })
            .await
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        let ids: Vec<(uuid::Uuid,)> =
            sqlx::query_as("SELECT service_account_id FROM known_service_accounts")
                .fetch_all(self.context.pool())
                .await?;
        ids.into_iter()
            .map(|(id,)| KvKey::new(service_account_kv_key(id)).map_err(DirectoryError::from))
            .collect()
    }
}
