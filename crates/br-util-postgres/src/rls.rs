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
/// - `app.current_user_id` — actor's UUID (user_id or service_account_id;
///   the impersonated user when impersonating, not the admin)
/// - `app.is_super_admin` — "true" or "false" (always "false" for Service)
/// - `app.is_active` — "true" or "false" (always "true" for Service)
/// - `app.is_pat` — "true" or "false" (always "false" for Service and JWT)
/// - `app.impersonator_id` — admin's UUID when impersonating, empty string
///   otherwise. Policies test `current_setting('app.impersonator_id') <> ''`.
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
    let is_pat = if passport.is_pat() { "true" } else { "false" };
    let impersonator_id = passport
        .impersonator_id()
        .map(|u| u.to_string())
        .unwrap_or_default();

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

    sqlx::query("SELECT set_config('app.is_pat', $1, true)")
        .bind(is_pat)
        .execute(&mut **tx)
        .await?;

    sqlx::query("SELECT set_config('app.impersonator_id', $1, true)")
        .bind(&impersonator_id)
        .execute(&mut **tx)
        .await?;

    Ok(())
}

/// Live-Postgres tests for `set_rls_context`.
///
/// Unlike role.rs / grant.rs (which verify DDL succeeds against the
/// production privilege model), these tests verify the *runtime* claim
/// of `set_rls_context`: that RLS policies referencing
/// `current_setting('app.current_user_id')` actually receive the
/// passport's actor_id and that the resulting row filtering is correct.
///
/// This is not testable with unit tests — sqlx mocks do not evaluate
/// policies, and `set_config(..., true)` semantics are transaction-bound
/// on a real session.
#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::grant::grant_app_access;
    use crate::role::ensure_app_role;
    use crate::test_support::{
        cleanup_role, open_pool_as, setup_caller, test_db_url, unique_role_name, unique_table_name,
    };
    use br_core_auth::{AuthMethod, Passport};
    use serde_json::json;
    use sqlx::Row;
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    const APP_PW: &str = "rls_test_app_pw";

    /// Minimal Human passport carrying just the actor_id. The RLS
    /// policies in these tests only depend on `app.current_user_id`,
    /// so the other fields are filler.
    fn passport_for(actor: Uuid) -> Passport {
        Passport::Human {
            user_id: actor,
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: json!({}),
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn set_rls_context_filters_rows_by_actor_id() {
        let Some(url) = test_db_url() else { return };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect as admin");
        let (owner_pool, owner) = setup_caller(&admin, &url).await;
        let app_role = unique_role_name();
        ensure_app_role(&owner_pool, &app_role, APP_PW)
            .await
            .expect("ensure app role");

        // Table with two rows owned by two distinct actors. Policy uses
        // `current_setting('app.current_user_id', true)` — the missing_ok
        // flag is what production policies use, so without
        // set_rls_context the predicate is `owner_id::text = ''` → 0 rows
        // instead of an error. That matches reality.
        let table = unique_table_name();
        for sql in [
            format!("CREATE TABLE \"{table}\" (id int, owner_id uuid NOT NULL, val text)"),
            format!("ALTER TABLE \"{table}\" ENABLE ROW LEVEL SECURITY"),
            format!(
                "CREATE POLICY actor_isolation ON \"{table}\" \
                 USING (owner_id::text = current_setting('app.current_user_id', true))"
            ),
        ] {
            sqlx::query(&sql)
                .execute(&owner_pool)
                .await
                .expect("schema setup");
        }
        let actor_a = Uuid::now_v7();
        let actor_b = Uuid::now_v7();
        // UUIDs are generated by uuid::now_v7 — well-formed, safe to
        // inline as quoted literals (the workspace's sqlx config doesn't
        // enable the uuid feature, so .bind(Uuid) won't typecheck).
        sqlx::query(&format!(
            "INSERT INTO \"{table}\" (id, owner_id, val) \
             VALUES (1, '{actor_a}', 'a'), (2, '{actor_b}', 'b')"
        ))
        .execute(&owner_pool)
        .await
        .expect("seed rows");

        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("grant_app_access");

        let app_pool = open_pool_as(&url, &app_role, APP_PW)
            .await
            .expect("app login");

        // Tx as actor_a: only row 1 visible.
        let mut tx = app_pool.begin().await.expect("tx a");
        set_rls_context(&mut tx, &passport_for(actor_a))
            .await
            .expect("set ctx a");
        let rows = sqlx::query(&format!("SELECT id FROM \"{table}\" ORDER BY id"))
            .fetch_all(&mut *tx)
            .await
            .expect("select a");
        assert_eq!(rows.len(), 1, "actor A must see exactly 1 row");
        let id: i32 = rows[0].try_get("id").expect("id");
        assert_eq!(id, 1);
        tx.commit().await.expect("commit a");

        // Tx as actor_b: only row 2 visible.
        let mut tx = app_pool.begin().await.expect("tx b");
        set_rls_context(&mut tx, &passport_for(actor_b))
            .await
            .expect("set ctx b");
        let rows = sqlx::query(&format!("SELECT id FROM \"{table}\" ORDER BY id"))
            .fetch_all(&mut *tx)
            .await
            .expect("select b");
        assert_eq!(rows.len(), 1, "actor B must see exactly 1 row");
        let id: i32 = rows[0].try_get("id").expect("id");
        assert_eq!(id, 2);
        tx.commit().await.expect("commit b");

        app_pool.close().await;
        owner_pool.close().await;
        cleanup_role(&admin, &app_role).await;
        cleanup_role(&admin, &owner).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn set_rls_context_does_not_leak_across_transactions() {
        // Guards the load-bearing `set_config(..., true)` third argument
        // in set_rls_context: switching the bool to false would persist
        // the actor identity across pooled connections and grant access
        // to whatever the previous request was scoped to. This test
        // proves that an unscoped tx (no set_rls_context call) sees 0
        // rows under the same policy that gave actor A 1 row above.
        let Some(url) = test_db_url() else { return };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect as admin");
        let (owner_pool, owner) = setup_caller(&admin, &url).await;
        let app_role = unique_role_name();
        ensure_app_role(&owner_pool, &app_role, APP_PW)
            .await
            .expect("ensure app role");

        let table = unique_table_name();
        for sql in [
            format!("CREATE TABLE \"{table}\" (id int, owner_id uuid NOT NULL)"),
            format!("ALTER TABLE \"{table}\" ENABLE ROW LEVEL SECURITY"),
            format!(
                "CREATE POLICY actor_isolation ON \"{table}\" \
                 USING (owner_id::text = current_setting('app.current_user_id', true))"
            ),
        ] {
            sqlx::query(&sql)
                .execute(&owner_pool)
                .await
                .expect("schema setup");
        }
        let actor_a = Uuid::now_v7();
        sqlx::query(&format!(
            "INSERT INTO \"{table}\" (id, owner_id) VALUES (1, '{actor_a}')"
        ))
        .execute(&owner_pool)
        .await
        .expect("seed");

        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("grant_app_access");

        // Pool max_connections=1 so the second tx MUST reuse the same
        // physical connection as the first — that's the only way to
        // observe a leak from local-config persistence.
        let app_pool = {
            use sqlx::postgres::PgConnectOptions;
            use std::str::FromStr;
            let opts = PgConnectOptions::from_str(&url)
                .expect("parse")
                .username(&app_role)
                .password(APP_PW);
            PgPoolOptions::new()
                .max_connections(1)
                .connect_with(opts)
                .await
                .expect("app login")
        };

        // Tx 1: set context for actor_a → sees their row.
        let mut tx = app_pool.begin().await.expect("tx 1");
        set_rls_context(&mut tx, &passport_for(actor_a))
            .await
            .expect("set ctx");
        let rows = sqlx::query(&format!("SELECT id FROM \"{table}\""))
            .fetch_all(&mut *tx)
            .await
            .expect("select 1");
        assert_eq!(rows.len(), 1);
        tx.commit().await.expect("commit 1");

        // Tx 2: same connection (max=1), no set_rls_context call → the
        // tx-local `app.current_user_id` from tx 1 must have been reset
        // at commit, so current_setting returns '' and the policy
        // filters everything out.
        let mut tx = app_pool.begin().await.expect("tx 2");
        let rows = sqlx::query(&format!("SELECT id FROM \"{table}\""))
            .fetch_all(&mut *tx)
            .await
            .expect("select 2");
        assert_eq!(
            rows.len(),
            0,
            "unscoped tx must see 0 rows; identity leak from tx 1 detected"
        );
        tx.commit().await.expect("commit 2");

        app_pool.close().await;
        owner_pool.close().await;
        cleanup_role(&admin, &app_role).await;
        cleanup_role(&admin, &owner).await;
    }
}
