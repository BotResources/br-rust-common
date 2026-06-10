//! [`migrate`] — apply this crate's `scope_registry` schema, explicitly.
//!
//! The migrations are **shipped by this crate** and embedded at compile time
//! (`sqlx::migrate!`). They are applied only when the composing service calls
//! [`migrate`] on boot, through its owner/migration pool
//! (`br_util_postgres::init_migration_pool`). This is **explicit invocation, not
//! auto-provisioning**: the schema objects are declared in `migrations/` and
//! applied by the operator's deliberate call — never created on-demand at
//! request time, and never `IF NOT EXISTS`-conjured by a runtime code path. A
//! service that never calls [`migrate`] runs against the schema as it finds it
//! and fails loud (a query against a missing table) rather than silently
//! provisioning it.

use sqlx::PgPool;

use crate::error::AppError;

/// Apply the scope-registry migrations against `pool` (must be connected as the
/// owner/migration role — migrations create tables the app role is later
/// granted access to via `br_util_postgres::grant_app_access`).
///
/// Idempotent: `sqlx`'s migrator records applied versions and skips them on a
/// re-run, so calling this on every boot is safe.
///
/// # Errors
///
/// [`AppError::Persistence`] if a migration fails to apply (a DDL fault or a
/// checksum mismatch against an already-applied version).
pub async fn migrate(pool: &PgPool) -> Result<(), AppError> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| AppError::Persistence(sqlx::Error::from(e)))?;
    Ok(())
}
