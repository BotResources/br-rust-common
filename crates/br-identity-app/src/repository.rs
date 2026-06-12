use br_core_scope::{ScopeSpec, ServiceManifest};
use br_identity_domain::ScopeRegistry;
use sqlx::{PgPool, Postgres, Transaction};

use crate::conflict::{SaveOutcome, classify_unique_violation};
use crate::error::AppError;
use crate::hydration::{ScopeRow, ServiceRow, build_hydration_input};

pub struct ScopeRegistryRepository {
    pool: PgPool,
}

impl ScopeRegistryRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub async fn load(&self) -> Result<(ScopeRegistry, i64), AppError> {
        let mut tx = self.pool.begin().await?;
        let (registry, version) = self.load_in_tx(&mut tx).await?;
        tx.commit().await?;
        Ok((registry, version))
    }

    pub async fn save(
        &self,
        registry: &ScopeRegistry,
        loaded_version: i64,
    ) -> Result<SaveOutcome, AppError> {
        let mut tx = self.pool.begin().await?;

        let new_version = registry.version() as i64;
        let advanced = sqlx::query(
            "UPDATE scope_registry_head SET version = $1 WHERE id = true AND version = $2",
        )
        .bind(new_version)
        .bind(loaded_version)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        if advanced == 0 {
            let head_exists: Option<i64> =
                sqlx::query_scalar("SELECT version FROM scope_registry_head WHERE id = true")
                    .fetch_optional(&mut *tx)
                    .await?;
            tx.rollback().await?;
            return match head_exists {
                Some(_) => Ok(SaveOutcome::VersionConflict),
                None => Err(AppError::MissingRegistryHead),
            };
        }

        for service in registry.services() {
            if let Err(conflict) = self.write_service(&mut tx, service).await? {
                tx.rollback().await?;
                return self.classify_scope_conflict(conflict).await;
            }
        }

        tx.commit().await?;
        Ok(SaveOutcome::Persisted)
    }

    async fn classify_scope_conflict(&self, scope_key: String) -> Result<SaveOutcome, AppError> {
        let owner: Option<String> =
            sqlx::query_scalar("SELECT owning_service FROM scope_registry WHERE scope_key = $1")
                .bind(&scope_key)
                .fetch_optional(&self.pool)
                .await?;
        match owner {
            Some(owner) => Ok(SaveOutcome::ScopeConflict { scope_key, owner }),
            None => Ok(SaveOutcome::VersionConflict),
        }
    }

    async fn load_in_tx(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<(ScopeRegistry, i64), AppError> {
        let version: Option<i64> =
            sqlx::query_scalar("SELECT version FROM scope_registry_head WHERE id = true")
                .fetch_optional(&mut **tx)
                .await?;
        let Some(version) = version else {
            return Err(AppError::MissingRegistryHead);
        };

        let service_rows: Vec<ServiceRow> = sqlx::query_as(
            "SELECT service_key, label_key, description_key FROM scope_registry_service \
             ORDER BY service_key",
        )
        .fetch_all(&mut **tx)
        .await?;

        let scope_rows: Vec<ScopeRow> = sqlx::query_as(
            "SELECT scope_key, owning_service, label_key, description_key, platform_only \
             FROM scope_registry ORDER BY owning_service, scope_key",
        )
        .fetch_all(&mut **tx)
        .await?;

        let services = build_hydration_input(service_rows, scope_rows)?;
        let registry = ScopeRegistry::hydrate(version as u64, services)?;
        Ok((registry, version))
    }

    async fn write_service(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        service: &br_identity_domain::RegisteredService,
    ) -> Result<Result<(), String>, AppError> {
        let manifest = service.manifest();
        sqlx::query(
            "INSERT INTO scope_registry_service (service_key, label_key, description_key) \
             VALUES ($1, $2, $3) ON CONFLICT (service_key) DO NOTHING",
        )
        .bind(manifest.key.as_str())
        .bind(&manifest.label_key)
        .bind(&manifest.description_key)
        .execute(&mut **tx)
        .await?;

        for spec in service.scopes() {
            if let Err(err) = self.write_scope(tx, manifest, spec).await {
                return match classify_unique_violation(&err, spec.key.as_str()) {
                    Some(key) => Ok(Err(key)),
                    None => Err(AppError::Persistence(err)),
                };
            }
        }
        Ok(Ok(()))
    }

    async fn write_scope(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        manifest: &ServiceManifest,
        spec: &ScopeSpec,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO scope_registry \
               (scope_key, owning_service, label_key, description_key, platform_only) \
             VALUES ($1, $2, $3, $4, $5) \
             ON CONFLICT (scope_key, owning_service) DO UPDATE SET last_seen_at = now()",
        )
        .bind(spec.key.as_str())
        .bind(manifest.key.as_str())
        .bind(&spec.label_key)
        .bind(&spec.description_key)
        .bind(spec.platform_only)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }
}
