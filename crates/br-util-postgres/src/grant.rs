use sqlx::PgPool;

use crate::error::PostgresError;

/// Grant table access to the application database role after migrations.
///
/// New tables created by migrations are owned by the migration role and
/// not accessible to the app role until explicitly granted.
///
/// The `app_role` parameter is project-specific (e.g., "hanshow_app",
/// "medisup_app"). Each project passes its own role name.
pub async fn grant_app_access(pool: &PgPool, app_role: &str) -> Result<(), PostgresError> {
    for sql in [
        format!("GRANT USAGE ON SCHEMA public TO {app_role}"),
        format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO {app_role}"
        ),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO {app_role}"),
    ] {
        sqlx::query(&sql)
            .execute(pool)
            .await
            .map_err(PostgresError::Db)?;
    }
    Ok(())
}
