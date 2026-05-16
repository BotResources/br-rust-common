use sqlx::PgPool;

use crate::error::PostgresError;
use crate::role::validate_role_name;

/// Grant table access to the application database role after migrations.
///
/// Tables created by migrations are owned by the migration role and not
/// accessible to the app role until explicitly granted. This function does
/// two things, both idempotent:
///
/// 1. **Bulk grant on existing objects** — `GRANT USAGE/SELECT/INSERT/
///    UPDATE/DELETE` on all tables and `USAGE, SELECT` on all sequences
///    currently in `public`.
/// 2. **Default privileges for future objects** — `ALTER DEFAULT PRIVILEGES
///    IN SCHEMA public GRANT … TO <app_role>` so that any *future* table or
///    sequence created by the current role (typically the owner running
///    later migrations) is automatically GRANTed to the app role. Without
///    this, a subsequent migration creating a new table would leave the app
///    role unable to touch it until a redeploy re-ran step 1.
///
/// **Must be invoked via the same pool that runs migrations.** Default
/// privileges attach to the role executing the statement (`CURRENT_USER`),
/// so calling this through the app pool would set defaults for objects the
/// app role creates — which it can't, making the call a silent no-op.
///
/// The `app_role` parameter is project-specific (e.g., "hanshow_app",
/// "medisup_app"). It is validated against `^[a-z][a-z0-9_]*$` (≤63 bytes)
/// before being interpolated into DDL; invalid names return
/// [`PostgresError::InvalidRoleName`] without touching the database.
pub async fn grant_app_access(pool: &PgPool, app_role: &str) -> Result<(), PostgresError> {
    validate_role_name(app_role)?;

    for sql in [
        format!("GRANT USAGE ON SCHEMA public TO \"{app_role}\""),
        format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO \"{app_role}\""
        ),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO \"{app_role}\""),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO \"{app_role}\""
        ),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT USAGE, SELECT ON SEQUENCES TO \"{app_role}\""
        ),
    ] {
        sqlx::query(&sql)
            .execute(pool)
            .await
            .map_err(PostgresError::Db)?;
    }
    Ok(())
}
