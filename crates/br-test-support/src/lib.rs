use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::future::Future;
use std::str::FromStr;
use std::time::Duration;
use uuid::Uuid;

pub const CALLER_PASSWORD: &str = "caller_pw_for_e2e_only";

const PUBLIC_SCHEMA_ACL_LOCK: i64 = 0x6272_5f74_6573_7431;
const CATALOG_RACE_ATTEMPTS: u32 = 20;
const CATALOG_RACE_BACKOFF: Duration = Duration::from_millis(25);

fn is_catalog_race(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(db) if matches!(db.code().as_deref(), Some("XX000" | "23505" | "42710"))
    )
}

async fn retry_catalog_race<F, Fut>(mut attempt: F) -> Result<(), sqlx::Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), sqlx::Error>>,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match attempt().await {
            Err(error) if attempts < CATALOG_RACE_ATTEMPTS && is_catalog_race(&error) => {
                tokio::time::sleep(CATALOG_RACE_BACKOFF).await;
            }
            other => return other,
        }
    }
}

pub fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

pub fn test_tls_db_url() -> Option<String> {
    std::env::var("TEST_TLS_DATABASE_URL").ok()
}

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

pub fn require_nats_url() -> String {
    nats_url().expect("NATS_URL is required for this ignored suite")
}

pub fn require_test_db_url() -> String {
    test_db_url().expect("TEST_DATABASE_URL is required for this ignored suite")
}

pub fn require_test_tls_db_url() -> String {
    test_tls_db_url().expect("TEST_TLS_DATABASE_URL is required for this ignored suite")
}

pub fn unique_suffix() -> String {
    Uuid::now_v7().simple().to_string()[..24].to_string()
}

pub fn unique_role_name() -> String {
    format!("br_test_{}", unique_suffix())
}

pub fn unique_table_name() -> String {
    format!("br_test_tbl_{}", unique_suffix())
}

pub struct IsolatedDatabase {
    pub name: String,
    pub url: String,
}

pub async fn create_isolated_database(admin: &PgPool, admin_url: &str) -> IsolatedDatabase {
    let name = unique_role_name();
    sqlx::query(&format!("CREATE DATABASE \"{name}\""))
        .execute(admin)
        .await
        .expect("create isolated database");
    let mut url = url::Url::parse(admin_url).expect("TEST_DATABASE_URL must parse as a URL");
    url.set_path(&format!("/{name}"));
    IsolatedDatabase {
        name,
        url: url.to_string(),
    }
}

pub async fn drop_isolated_database(admin: &PgPool, database: &IsolatedDatabase) {
    sqlx::query(&format!(
        "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
        database.name
    ))
    .execute(admin)
    .await
    .expect("drop isolated database");
}

pub async fn cleanup_role(admin: &PgPool, role: &str) {
    let drop_owned = format!("DROP OWNED BY \"{role}\" CASCADE");
    let _ =
        retry_catalog_race(|| async { sqlx::query(&drop_owned).execute(admin).await.map(|_| ()) })
            .await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
        .execute(admin)
        .await;
}

pub async fn setup_caller(admin: &PgPool, admin_url: &str) -> (PgPool, String) {
    let caller = unique_role_name();
    let password = CALLER_PASSWORD;

    let create_sql = format!(
        "CREATE ROLE \"{caller}\" LOGIN CREATEROLE NOSUPERUSER \
         PASSWORD '{password}'"
    );
    sqlx::query(&create_sql)
        .execute(admin)
        .await
        .expect("create caller role");

    let grant_create = format!("GRANT CREATE ON SCHEMA public TO \"{caller}\"");
    retry_catalog_race(|| async {
        let mut tx = admin.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(PUBLIC_SCHEMA_ACL_LOCK)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&grant_create).execute(&mut *tx).await?;
        tx.commit().await
    })
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
