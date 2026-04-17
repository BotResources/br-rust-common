use br_core_auth::Passport;
use sqlx::Postgres;

/// Inject Passport fields into Postgres session variables for RLS policies.
///
/// Uses `set_config(..., true)` so values are **transaction-local** — they
/// reset automatically at commit or rollback. This eliminates RLS identity
/// leakage on pooled connections: no manual cleanup is needed.
///
/// **Requires an explicit transaction.** Calling this outside a transaction
/// has no lasting effect (values are discarded immediately).
///
/// ```ignore
/// let mut tx = pool.begin().await?;
/// set_rls_context(&mut tx, &passport).await?;
/// let row = sqlx::query("SELECT ...").fetch_one(&mut *tx).await?;
/// tx.commit().await?;
/// ```
///
/// Variables set:
/// - `app.current_user_id` — actor's UUID (user_id or service_account_id)
/// - `app.is_super_admin` — "true" or "false" (always "false" for Service)
/// - `app.is_active` — "true" or "false" (always "true" for Service)
pub async fn set_rls_context(
    tx: &mut sqlx::Transaction<'_, Postgres>,
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

    sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
        .bind(&current_user_id)
        .execute(&mut **tx)
        .await?;

    sqlx::query("SELECT set_config('app.is_super_admin', $1, true)")
        .bind(is_super_admin)
        .execute(&mut **tx)
        .await?;

    sqlx::query("SELECT set_config('app.is_active', $1, true)")
        .bind(is_active)
        .execute(&mut **tx)
        .await?;

    Ok(())
}
