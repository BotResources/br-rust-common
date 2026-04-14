use crate::passport::Passport;

/// Inject Passport fields into Postgres session variables for RLS policies.
///
/// Uses connection-level scope (`false`), not transaction-local (`true`),
/// because GraphQL resolvers operate in autocommit mode — transaction-local
/// settings would be lost between the `set_config` call and the subsequent
/// query. The connection is acquired per-resolver and returned to the pool
/// after the handler completes, so there is no cross-request leakage.
///
/// Variables set:
/// - `app.current_user_id` — actor's UUID (user_id or service_account_id)
/// - `app.is_super_admin` — "true" or "false" (always "false" for Service)
/// - `app.is_active` — "true" or "false" (always "true" for Service)
pub async fn set_rls_context(
    conn: &mut sqlx::PgConnection,
    passport: &Passport,
) -> Result<(), sqlx::Error> {
    let current_user_id = passport.actor_id().to_string();
    let is_super_admin = if passport.is_super_admin() {
        "true"
    } else {
        "false"
    };
    let is_active = if passport.is_active() {
        "true"
    } else {
        "false"
    };

    sqlx::query("SELECT set_config('app.current_user_id', $1, false)")
        .bind(&current_user_id)
        .execute(&mut *conn)
        .await?;

    sqlx::query("SELECT set_config('app.is_super_admin', $1, false)")
        .bind(is_super_admin)
        .execute(&mut *conn)
        .await?;

    sqlx::query("SELECT set_config('app.is_active', $1, false)")
        .bind(is_active)
        .execute(&mut *conn)
        .await?;

    Ok(())
}
