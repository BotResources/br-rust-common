use sqlx::PgPool;

use crate::passport::Passport;

/// Inject Passport fields into Postgres session variables for RLS policies.
///
/// Uses `set_config(..., false)` so the setting persists for the connection
/// lifetime (not just the current transaction). This is required because
/// queries typically execute in autocommit mode — `set_config(..., true)`
/// values would be lost between the set_config call and subsequent queries.
///
/// **Contract:** Every pooled connection must call `set_rls_context` before
/// any RLS-protected query. Prefer [`with_rls`] over calling this directly
/// to enforce this pattern and guarantee cleanup.
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

/// Acquire a connection from the pool, set RLS context, run a closure, then
/// clear the RLS variables before returning the connection to the pool.
///
/// The closure receives `&mut PgConnection` (borrowed, not owned) so that
/// `with_rls` retains ownership and can always run cleanup afterward —
/// even if the closure returns an error.
///
/// If RLS cleanup fails, the connection is marked via `close_on_drop()` so
/// sqlx closes it on drop instead of recycling it back to the pool.
///
/// ```ignore
/// use futures::FutureExt;
///
/// let count = with_rls(&pool, &passport, |conn| async move {
///     db::unread_count(conn).await
/// }.boxed()).await?;
/// ```
pub async fn with_rls<F, T, E>(
    pool: &PgPool,
    passport: &Passport,
    f: F,
) -> Result<T, E>
where
    F: for<'c> FnOnce(&'c mut sqlx::PgConnection) -> futures::future::BoxFuture<'c, Result<T, E>>,
    E: From<sqlx::Error>,
{
    let mut conn = pool.acquire().await.map_err(E::from)?;
    set_rls_context(&mut conn, passport).await.map_err(E::from)?;

    let result = f(&mut conn).await;

    // Always attempt to clear RLS before the connection returns to the pool.
    // If cleanup fails, mark the connection for close-on-drop so sqlx
    // destroys it instead of recycling it with stale RLS identity.
    if let Err(e) = clear_rls_context(&mut conn).await {
        tracing::warn!(error = %e, "failed to clear RLS context — connection will be closed on drop");
        conn.close_on_drop();
    }

    result
}

/// Clear RLS session variables on a connection.
///
/// Resets `app.current_user_id` to the nil UUID (`00000000-...`). This is a
/// valid UUID that will not match any real user ID, so RLS policies will
/// deny access — a safe default that prevents accidental data exposure.
///
/// `is_super_admin` and `is_active` are reset to `"false"`.
///
/// Call this before returning a connection to the pool if you are managing
/// connections manually (not using [`with_rls`]).
pub async fn clear_rls_context(conn: &mut sqlx::PgConnection) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('app.current_user_id', '00000000-0000-0000-0000-000000000000', false)")
        .execute(&mut *conn)
        .await?;
    sqlx::query("SELECT set_config('app.is_super_admin', 'false', false)")
        .execute(&mut *conn)
        .await?;
    sqlx::query("SELECT set_config('app.is_active', 'false', false)")
        .execute(&mut *conn)
        .await?;
    Ok(())
}
