use std::str::FromStr;

use br_test_support::{cleanup_role, require_test_db_url, setup_caller, unique_role_name};
use br_util_postgres::{PostgresError, ensure_app_role};
use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
use sqlx::{Connection, PgPool};
use uuid::Uuid;

fn generated_password() -> String {
    format!("pw_{}", Uuid::now_v7().simple())
}

async fn try_login(
    admin_url: &str,
    role: &str,
    password: &str,
) -> Result<PgConnection, sqlx::Error> {
    let options = PgConnectOptions::from_str(admin_url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(role)
        .password(password);
    PgConnection::connect_with(&options).await
}

async fn assert_login_accepted(url: &str, role: &str, password: &str) {
    try_login(url, role, password)
        .await
        .expect("login must be accepted")
        .close()
        .await
        .ok();
}

async fn assert_login_refused(url: &str, role: &str, password: &str) {
    assert!(
        try_login(url, role, password).await.is_err(),
        "login must be refused"
    );
}

async fn stored_password(admin: &PgPool, role: &str) -> String {
    sqlx::query_scalar("SELECT rolpassword FROM pg_authid WHERE rolname = $1")
        .bind(role)
        .fetch_one(admin)
        .await
        .expect("read pg_authid")
}

async fn admin_pool(url: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(2)
        .connect(url)
        .await
        .expect("connect as admin")
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn role_logs_in_with_its_password_and_refuses_a_wrong_one() {
    let url = require_test_db_url();
    let admin = admin_pool(&url).await;
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    let role = unique_role_name();
    let password = generated_password();

    ensure_app_role(&caller_pool, &role, &password)
        .await
        .expect("ensure_app_role");

    assert_login_accepted(&url, &role, &password).await;
    assert_login_refused(&url, &role, &generated_password()).await;

    caller_pool.close().await;
    cleanup_role(&admin, &role).await;
    cleanup_role(&admin, &caller).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn stored_password_is_a_scram_verifier_without_the_clear_password() {
    let url = require_test_db_url();
    let admin = admin_pool(&url).await;
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    let role = unique_role_name();
    let password = generated_password();

    ensure_app_role(&caller_pool, &role, &password)
        .await
        .expect("ensure_app_role");

    let stored = stored_password(&admin, &role).await;
    assert!(stored.starts_with("SCRAM-SHA-256$4096:"), "stored={stored}");
    assert!(!stored.contains(&password));

    caller_pool.close().await;
    cleanup_role(&admin, &role).await;
    cleanup_role(&admin, &caller).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn calling_again_with_a_new_password_rotates_it() {
    let url = require_test_db_url();
    let admin = admin_pool(&url).await;
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    let role = unique_role_name();
    let old_password = generated_password();
    let new_password = generated_password();

    ensure_app_role(&caller_pool, &role, &old_password)
        .await
        .expect("set old");
    assert_login_accepted(&url, &role, &old_password).await;

    ensure_app_role(&caller_pool, &role, &new_password)
        .await
        .expect("rotate");
    assert_login_accepted(&url, &role, &new_password).await;
    assert_login_refused(&url, &role, &old_password).await;

    caller_pool.close().await;
    cleanup_role(&admin, &role).await;
    cleanup_role(&admin, &caller).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn calling_again_with_the_same_password_keeps_it_working() {
    let url = require_test_db_url();
    let admin = admin_pool(&url).await;
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    let role = unique_role_name();
    let password = generated_password();

    ensure_app_role(&caller_pool, &role, &password)
        .await
        .expect("first call");
    ensure_app_role(&caller_pool, &role, &password)
        .await
        .expect("second call");

    assert_login_accepted(&url, &role, &password).await;
    assert_login_refused(&url, &role, &generated_password()).await;

    caller_pool.close().await;
    cleanup_role(&admin, &role).await;
    cleanup_role(&admin, &caller).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn password_with_quotes_and_dollars_logs_in() {
    let url = require_test_db_url();
    let admin = admin_pool(&url).await;
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    let role = unique_role_name();
    let password = format!("weird $ pw $$ with $foo$ and 'quotes' {}", Uuid::now_v7());

    ensure_app_role(&caller_pool, &role, &password)
        .await
        .expect("special-char password must be set");

    assert_login_accepted(&url, &role, &password).await;
    assert!(!stored_password(&admin, &role).await.contains(&password));

    caller_pool.close().await;
    cleanup_role(&admin, &role).await;
    cleanup_role(&admin, &caller).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn empty_password_is_refused_before_any_statement_is_sent() {
    let url = require_test_db_url();
    let admin = admin_pool(&url).await;
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    let role = unique_role_name();

    let result = ensure_app_role(&caller_pool, &role, "").await;

    assert!(matches!(result, Err(PostgresError::Config(_))));
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(&role)
            .fetch_one(&admin)
            .await
            .expect("role lookup");
    assert!(!exists);

    caller_pool.close().await;
    cleanup_role(&admin, &caller).await;
}
