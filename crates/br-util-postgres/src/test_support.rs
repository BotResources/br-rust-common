use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;

pub fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

pub fn test_tls_db_url() -> Option<String> {
    std::env::var("TEST_TLS_DATABASE_URL").ok()
}

pub fn unique_role_name() -> String {
    let suffix = &Uuid::now_v7().simple().to_string()[..24];
    format!("br_test_{suffix}")
}

pub fn unique_table_name() -> String {
    let suffix = &Uuid::now_v7().simple().to_string()[..24];
    format!("br_test_tbl_{suffix}")
}

pub async fn cleanup_role(admin: &PgPool, role: &str) {
    let _ = sqlx::query(&format!("DROP OWNED BY \"{role}\" CASCADE"))
        .execute(admin)
        .await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
        .execute(admin)
        .await;
}

pub async fn setup_caller(admin: &PgPool, admin_url: &str) -> (PgPool, String) {
    let caller = unique_role_name();
    let password = "caller_pw_for_e2e_only";

    let create_sql = format!(
        "CREATE ROLE \"{caller}\" LOGIN CREATEROLE NOSUPERUSER \
         PASSWORD '{password}'"
    );
    sqlx::query(&create_sql)
        .execute(admin)
        .await
        .expect("create caller role");

    sqlx::query(&format!("GRANT CREATE ON SCHEMA public TO \"{caller}\""))
        .execute(admin)
        .await
        .expect("grant CREATE on public to caller");

    let opts = PgConnectOptions::from_str(admin_url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(&caller)
        .password(password);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(opts)
        .await
        .expect("connect as caller");

    (pool, caller)
}

pub async fn open_pool_as(
    admin_url: &str,
    role: &str,
    password: &str,
) -> Result<PgPool, sqlx::Error> {
    let opts = PgConnectOptions::from_str(admin_url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(role)
        .password(password);
    PgPoolOptions::new()
        .max_connections(2)
        .connect_with(opts)
        .await
}
