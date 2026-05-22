//! Full-bootstrap integration test: walks the exact sequence that a
//! `svc-*` service runs on first boot of a fresh database. Per-module
//! live tests in src/{role,grant,rls}.rs exercise each function in
//! isolation; this test catches seam bugs between them — the kind of
//! regression that broke dp-botresources.ai PR #37 on first boot of the
//! BR 2-role data plane.
//!
//! Production sequence (mirrored here):
//!   1. owner pool = LOGIN CREATEROLE NOSUPERUSER (CNPG's `<svc>_owner`)
//!   2. `ensure_app_role(owner, "<svc>_app", pw)` — idempotent create
//!   3. migrate as owner (here: hand-crafted CREATE TABLE + policy)
//!   4. `grant_app_access(owner, "<svc>_app")` — SELECT/INSERT/UPDATE/
//!      DELETE + ALTER DEFAULT PRIVILEGES for future tables
//!   5. open app pool (LOGIN, NOSUPERUSER, no other attributes)
//!   6. per-request: BEGIN, `set_rls_context(tx, passport)`, query,
//!      COMMIT
//!
//! The test asserts the end-to-end claim: a row inserted as actor A is
//! visible only to actor A through the app pool's RLS-protected SELECT.
//! Any silent break in steps 1-5 makes step 6 fail or wrong.

mod common;

use br_core_auth::{AuthMethod, Passport};
use br_util_postgres::{ensure_app_role, grant_app_access, set_rls_context};
use serde_json::json;
use sqlx::Row;
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

use common::{
    cleanup_role, open_pool_as, setup_owner, test_db_url, unique_role_name, unique_table_name,
};

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

    // Step 0: admin (superuser) connection. Only used to bootstrap the
    // owner and to clean up at the end — production never has a live
    // superuser pool in the service process.
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect as admin");

    // Step 1: owner pool, mirrors CNPG's `<svc>_owner`.
    let (owner_pool, owner) = setup_owner(&admin, &url).await;
    let app_role = unique_role_name();

    // Step 2: create the app role through the owner pool. Hits the
    // PG-16 NOSUPERUSER-assertion code path that Scenario 1 of #13
    // broke (verified by the live tests in role.rs too).
    ensure_app_role(&owner_pool, &app_role, APP_PW)
        .await
        .expect("ensure_app_role");

    // Step 3: "migrate" as owner — create the RLS-protected table.
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

    // Step 4: grant the app role table/sequence access.
    grant_app_access(&owner_pool, &app_role)
        .await
        .expect("grant_app_access");

    // Step 5: switch to the app pool — the long-lived runtime pool.
    let app_pool = open_pool_as(&url, &app_role, APP_PW)
        .await
        .expect("app login");

    // Step 6: per-request flow. Actor A sees their row; actor B sees
    // theirs; neither sees the other's. The full chain only "works" if
    // every previous step did its job correctly — a missed grant, a
    // wrong set_config tag, a typo in a policy variable name would
    // collapse into "0 rows" or "permission denied" here.
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
