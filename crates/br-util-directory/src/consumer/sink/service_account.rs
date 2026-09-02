use std::collections::BTreeSet;

use br_core_directory::{
    PublishedServiceAccount, service_account_id_from_kv_key, service_account_kv_key,
};
use br_util_nats_fabric::{KvKey, ProjectionSink};

use crate::consumer::sink::context::SinkContext;
use crate::error::DirectoryError;
use crate::impact::ForeignRef;

const UPSERT: &str = "INSERT INTO known_service_accounts AS t (service_account_id, name) \
     VALUES ($1, $2) \
     ON CONFLICT (service_account_id) DO UPDATE SET name = EXCLUDED.name \
     WHERE t.name IS DISTINCT FROM EXCLUDED.name";

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

        let mut tx = self.context.pool().begin().await?;
        let changed = sqlx::query(UPSERT)
            .bind(service_account_id)
            .bind(&value.name)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            > 0;
        if changed {
            self.context
                .stage_change(&mut tx, || ForeignRef::service_account(service_account_id))
                .await?;
        }
        tx.commit().await?;

        if changed {
            self.context.record_change();
        }
        Ok(())
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(service_account_id) = service_account_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        let mut tx = self.context.pool().begin().await?;
        let deleted =
            sqlx::query("DELETE FROM known_service_accounts WHERE service_account_id = $1")
                .bind(service_account_id)
                .execute(&mut *tx)
                .await?
                .rows_affected()
                > 0;
        if deleted {
            self.context
                .stage_change(&mut tx, || ForeignRef::service_account(service_account_id))
                .await?;
        }
        tx.commit().await?;

        if deleted {
            self.context.record_change();
        }
        Ok(())
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
