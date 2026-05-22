//! Test helpers used by the integration tests in `tests/`. Mirrors
//! src/test_support.rs (which lives at crate root, `#[cfg(test)]`-gated,
//! so it is not reachable from this separate test binary). Kept in sync
//! by hand — the surface is small.

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;

pub fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
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

pub async fn setup_owner(admin: &PgPool, admin_url: &str) -> (PgPool, String) {
    let owner = unique_role_name();
    let password = "owner_pw_for_e2e_only";

    sqlx::query(&format!(
        "CREATE ROLE \"{owner}\" LOGIN CREATEROLE NOSUPERUSER \
         PASSWORD '{password}'"
    ))
    .execute(admin)
    .await
    .expect("create owner role");

    // PG 15+ revoked default CREATE-on-public from PUBLIC. CNPG's
    // <svc>_owner has it via database ownership; here we grant it
    // explicitly so the bootstrap chain can CREATE TABLE in public.
    sqlx::query(&format!("GRANT CREATE ON SCHEMA public TO \"{owner}\""))
        .execute(admin)
        .await
        .expect("grant CREATE on public to owner");

    let opts = PgConnectOptions::from_str(admin_url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(&owner)
        .password(password);
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(opts)
        .await
        .expect("connect as owner");

    (pool, owner)
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
