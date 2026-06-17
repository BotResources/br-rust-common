use br_core_auth::Passport;
use sqlx::Postgres;

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

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::grant::grant_app_access;
    use crate::role::ensure_app_role;
    use br_core_auth::{AuthMethod, Passport, PassportClaims};
    use br_test_support::{
        cleanup_role, open_pool_as, setup_caller, test_db_url, unique_role_name, unique_table_name,
    };
    use sqlx::Row;
    use sqlx::postgres::PgPoolOptions;
    use uuid::Uuid;

    const APP_PW: &str = "rls_test_app_pw";

    fn passport_for(actor: Uuid) -> Passport {
        Passport::human(
            actor,
            false,
            true,
            AuthMethod::Jwt,
            None,
            PassportClaims::new(),
        )
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
