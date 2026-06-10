//! [`ScopeRegistryRepository`] — the Postgres adapter that loads and saves the
//! [`ScopeRegistry`](br_identity_domain::ScopeRegistry) singleton. Postgres is
//! the source of truth: the registry is persisted as current-state rows across
//! three tables (see `migrations/`) — a one-row `scope_registry_head` holding
//! the singleton's optimistic-lock `version`, `scope_registry_service` for
//! manifests, and `scope_registry` for owned scopes (with the global
//! `UNIQUE(scope_key)` net).
//!
//! ## Optimistic locking on a singleton
//!
//! One `version` on the one-row head table guards the whole aggregate (not a
//! per-row version): the uniqueness invariant spans every service.
//! [`save`](ScopeRegistryRepository::save) conditions its head
//! `UPDATE … WHERE version = $loaded`; a zero-row update means another writer
//! advanced it first → [`SaveOutcome::VersionConflict`], which the pipeline
//! retries (re-hydrate, re-judge).
//!
//! ## The two conflicts are different verdicts
//!
//! - A **version conflict** is a benign race: retry it.
//! - A **`UNIQUE(scope_key)` violation** is the database enforcing the
//!   aggregate's uniqueness invariant as the final net. It maps to
//!   [`SaveOutcome::ScopeConflict`] → the pipeline turns it into a `rejected`
//!   confirmation, **never** a nak (which would redeliver, re-violate, and loop
//!   forever). This adapter classifies the violation and returns the contested
//!   key rather than erroring.

use br_core_scope::{ScopeSpec, ServiceManifest};
use br_identity_domain::ScopeRegistry;
use sqlx::{PgPool, Postgres, Transaction};

use crate::conflict::{SaveOutcome, classify_unique_violation};
use crate::error::AppError;
use crate::hydration::{ScopeRow, ServiceRow, build_hydration_input};

/// Loads and saves the scope registry against a Postgres pool.
///
/// Holds the runtime app pool (least-privilege role: SELECT/INSERT/UPDATE on the
/// three registry tables, no RLS — see the migration). The pool's role is an
/// adapter/composition concern; the domain never sees it.
pub struct ScopeRegistryRepository {
    pool: PgPool,
}

impl ScopeRegistryRepository {
    /// Bind the repository to a pool. The pool is assumed already validated and
    /// connected by the composing service (`br_util_postgres::init_pool`).
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Load the registry, hydrating the aggregate and re-validating every
    /// invariant (the read-side double barrier). Returns the rebuilt aggregate
    /// and the loaded `version` for the optimistic-lock save.
    ///
    /// # Errors
    ///
    /// - [`AppError::MissingRegistryHead`] if the singleton head row is absent
    ///   (a migration/bootstrap fault — fail loud, never re-create it);
    /// - [`AppError::Hydration`] if the persisted rows form a state the domain
    ///   considers impossible;
    /// - [`AppError::Persistence`] on a query/transport fault.
    pub async fn load(&self) -> Result<(ScopeRegistry, i64), AppError> {
        let mut tx = self.pool.begin().await?;
        let (registry, version) = self.load_in_tx(&mut tx).await?;
        // A read-only load: commit (no writes) to release the snapshot cleanly.
        tx.commit().await?;
        Ok((registry, version))
    }

    /// Persist the judged registry, conditioned on the loaded `version`
    /// (optimistic lock). Idempotent and replayable: services are upserted,
    /// scopes are inserted with a touch of `last_seen_at`, and the head version
    /// advances only when it still equals `loaded_version`.
    ///
    /// Returns the [`SaveOutcome`]: `Persisted`, a benign `VersionConflict` (the
    /// caller retries), or a terminal `ScopeConflict { scope_key, owner }` (the
    /// caller rejects). A unique-violation is classified here, not raised as an
    /// error; the contested key's **actual** owner is read back from the
    /// committed winner's row (if that row has since vanished — the conflicting
    /// writer rolled back — the conflict is downgraded to a `VersionConflict` so
    /// the caller re-judges rather than rejecting against a fabricated owner).
    ///
    /// # Errors
    ///
    /// [`AppError::Persistence`] on any non-unique-violation query/transport
    /// fault; [`AppError::MissingRegistryHead`] if the head row vanished.
    pub async fn save(
        &self,
        registry: &ScopeRegistry,
        loaded_version: i64,
    ) -> Result<SaveOutcome, AppError> {
        let mut tx = self.pool.begin().await?;

        // Advance the head version, conditioned on the loaded value. A zero-row
        // update means a concurrent writer already advanced it → version
        // conflict. Do this first so a losing writer never even attempts the
        // row writes (and so it cannot raise a misleading unique violation).
        //
        // The aggregate's version is a `u64`; `bigint` is `i64`. The version is
        // monotonic from 0 and bumps once per accepted command, so it cannot
        // realistically reach `i64::MAX`; the column's `CHECK (version >= 0)`
        // also rejects a wrapped negative on the DB side.
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
            // Distinguish "head missing" (fail loud) from "version moved" (retry):
            // probe the head once.
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

        // Upsert each service and its scopes from the judged aggregate. The
        // aggregate's full current state is written idempotently; a redelivery
        // or reconnect-replay applies the same rows without doubling.
        for service in registry.services() {
            if let Err(conflict) = self.write_service(&mut tx, service).await? {
                // Unique violation on a scope key: terminal rejection, not a
                // nak. Roll back the whole attempt, then read back the actual
                // owner (the committed winner) so the rejection names the truth.
                tx.rollback().await?;
                return self.classify_scope_conflict(conflict).await;
            }
        }

        tx.commit().await?;
        Ok(SaveOutcome::Persisted)
    }

    /// Turn a rolled-back `UNIQUE(scope_key)` violation into the verdict the
    /// pipeline acts on. Reads back the **actual** owner of the contested key —
    /// the committed winner of the race, never the losing declarant.
    ///
    /// Edge case — the winning row is gone by the time we read it (the
    /// conflicting writer rolled back, or its row was deleted): that is a
    /// transient race, not a settled conflict, so fall back to
    /// [`SaveOutcome::VersionConflict`] (the pipeline re-hydrates and re-judges)
    /// rather than fabricating an owner.
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

    /// Hydrate within an open transaction: read the head version, the service
    /// rows, and the scope rows under one snapshot, then rebuild the aggregate
    /// (re-validating every invariant via [`ScopeRegistry::hydrate`]).
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

    /// Upsert one service's manifest, then insert each scope it owns (touching
    /// `last_seen_at`). Returns `Ok(Err(scope_key))` when a `UNIQUE(scope_key)`
    /// violation is hit (a terminal conflict the caller maps to a rejection);
    /// `Ok(Ok(()))` on success; `Err` on any other persistence fault.
    async fn write_service(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        service: &br_identity_domain::RegisteredService,
    ) -> Result<Result<(), String>, AppError> {
        let manifest = service.manifest();
        // Manifest upsert: idempotent on re-declaration. The stored label/desc
        // keys are left as first written (idempotency is keyed on identity, not
        // metadata — matching the domain's no-op-on-metadata-change rule).
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

    /// Insert (or touch) one scope row, idempotently, while making a cross-owner
    /// conflict raise.
    ///
    /// The upsert's arbiter is the **composite** `(scope_key, owning_service)`
    /// unique index (see the migration):
    ///
    /// - a **same-owner** re-declare conflicts on that arbiter → `DO UPDATE`
    ///   touches `last_seen_at` only; `registered_at` is never overwritten;
    /// - a **cross-owner** declare conflicts on the `scope_key` PRIMARY KEY,
    ///   which the `ON CONFLICT` clause does **not** name, so Postgres raises a
    ///   unique violation (SQLSTATE 23505) rather than silently no-op'ing. That
    ///   raised violation is the final net the caller classifies into a terminal
    ///   rejection — never a nak.
    ///
    /// (A plain `ON CONFLICT (scope_key) DO UPDATE … WHERE owner = …` would
    /// *silently no-op* a cross-owner conflict instead of raising — the
    /// composite-arbiter design is what makes the net actually fire.)
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
