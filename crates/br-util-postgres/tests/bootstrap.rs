use br_core_auth::{AuthMethod, Passport};
use br_test_support::{
    cleanup_role, open_pool_as, setup_caller, test_db_url, unique_role_name, unique_table_name,
};
use br_util_postgres::{ensure_app_role, grant_app_access, set_rls_context};
use serde_json::json;
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

const APP_PW: &str = "bootstrap_app_pw_e2e_only";

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
async fn full_bootstrap_chain_isolates_rows_per_actor() {
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
        .expect("ensure_app_role");

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
            .expect("migration step");
    }
    let actor_a = Uuid::now_v7();
    let actor_b = Uuid::now_v7();
    sqlx::query(&format!(
        "INSERT INTO \"{table}\" (id, owner_id, val) \
         VALUES (1, '{actor_a}', 'a'), (2, '{actor_b}', 'b')"
    ))
    .execute(&owner_pool)
    .await
    .expect("seed");

    grant_app_access(&owner_pool, &app_role)
        .await
        .expect("grant_app_access");

    let app_pool = open_pool_as(&url, &app_role, APP_PW)
        .await
        .expect("app login");

    for (actor, expected_id) in [(actor_a, 1i32), (actor_b, 2)] {
        let mut tx = app_pool.begin().await.expect("begin tx");
        set_rls_context(&mut tx, &passport_for(actor))
            .await
            .expect("set_rls_context");
        let rows = sqlx::query(&format!("SELECT id FROM \"{table}\" ORDER BY id"))
            .fetch_all(&mut *tx)
            .await
            .expect("SELECT under RLS");
        assert_eq!(
            rows.len(),
            1,
            "actor {actor} must see exactly their one row"
        );
        let id: i32 = rows[0].try_get("id").expect("id col");
        assert_eq!(id, expected_id);
        tx.commit().await.expect("commit");
    }

    app_pool.close().await;
    owner_pool.close().await;
    cleanup_role(&admin, &app_role).await;
    cleanup_role(&admin, &owner).await;
}
