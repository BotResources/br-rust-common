use std::collections::BTreeSet;

use br_core_directory::{
    PublishedServiceAccount, service_account_id_from_kv_key, service_account_kv_key,
};
use br_util_nats_fabric::{KvKey, ProjectionSink};
use sqlx::PgPool;

use crate::error::DirectoryError;

pub(crate) struct ServiceAccountSink {
    pool: PgPool,
}

impl ServiceAccountSink {
    pub(crate) fn new(pool: PgPool) -> Self {
        Self { pool }
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
        sqlx::query(
            "INSERT INTO known_service_accounts (service_account_id, name) \
             VALUES ($1, $2) \
             ON CONFLICT (service_account_id) DO UPDATE SET name = EXCLUDED.name",
        )
        .bind(service_account_id)
        .bind(&value.name)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(service_account_id) = service_account_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };
        sqlx::query("DELETE FROM known_service_accounts WHERE service_account_id = $1")
            .bind(service_account_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        let ids: Vec<(uuid::Uuid,)> =
            sqlx::query_as("SELECT service_account_id FROM known_service_accounts")
                .fetch_all(&self.pool)
                .await?;
        ids.into_iter()
            .map(|(id,)| KvKey::new(service_account_kv_key(id)).map_err(DirectoryError::from))
            .collect()
    }
}
