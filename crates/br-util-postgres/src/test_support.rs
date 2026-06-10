//! Shared helpers for the `#[ignore]`d live-Postgres tests in role.rs,
//! grant.rs, and rls.rs. Lives at crate root, `#[cfg(test)]`-gated so it
//! compiles only for the test build and never ships in the library.
//!
//! All e2e tests follow the same shape:
//!   1. Connect as admin (TEST_DATABASE_URL, expected superuser).
//!   2. `setup_caller` to bootstrap a non-superuser CREATEROLE role — that
//!      caller becomes the "owner" pool that runs ensure_app_role / DDL /
//!      grant_app_access, mirroring CNPG's `<svc>_owner` in production.
//!   3. Test body.
//!   4. `cleanup_role` to drop everything owned by, then drop, each test
//!      role — in reverse-dependency order (app role → owner role).
//!
//! Bootstrapping the non-superuser caller is what catches Scenario 1 of
//! issue #13 — calling helpers through the SUPERUSER admin pool would
//! silently mask PG 16+ privilege rejections.

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;

/// Returns `Some(url)` only when the live-test prerequisite env var is
/// set; `None` skips silently so the test body can early-return when
/// developers run the full suite without provisioning a DB.
pub fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

/// Returns `Some(url)` only when a **TLS-enabled** Postgres is provisioned
/// for the full-handshake proof test (a server started with `ssl=on` and a
/// cert). `None` skips silently — TLS-server provisioning is heavier than a
/// plain `postgres:16-alpine`, so the handshake test stays opt-in and the
/// rest of the live suite runs against `TEST_DATABASE_URL` (plaintext).
pub fn test_tls_db_url() -> Option<String> {
    std::env::var("TEST_TLS_DATABASE_URL").ok()
}

/// Unique role name per test invocation so parallel runs do not collide,
/// and so a stale role from a crashed run does not pollute the next.
/// Matches the `^[a-z][a-z0-9_]*$` validator used by `ensure_app_role`.
pub fn unique_role_name() -> String {
    // UUID v7 simple = 32 lowercase hex digits; prefix with a letter to
    // satisfy the validator. Truncated to stay well under the 63-byte cap.
    let suffix = &Uuid::now_v7().simple().to_string()[..24];
    format!("br_test_{suffix}")
}

/// Unique unquoted table identifier safe to interpolate into DDL.
pub fn unique_table_name() -> String {
    let suffix = &Uuid::now_v7().simple().to_string()[..24];
    format!("br_test_tbl_{suffix}")
}

/// Best-effort cleanup of a test role and any objects it owns. Must be
/// called as superuser (admin pool). `DROP OWNED` is required before
/// `DROP ROLE` whenever the role created tables/sequences/etc., which is
/// the common case for the owner role in grant/rls tests.
pub async fn cleanup_role(admin: &PgPool, role: &str) {
    // DROP OWNED needs the role to exist; if it doesn't, both queries
    // silently succeed.
    let _ = sqlx::query(&format!("DROP OWNED BY \"{role}\" CASCADE"))
        .execute(admin)
        .await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
        .execute(admin)
        .await;
}

/// Bootstrap a fresh `caller_<uuid>` role with the production privilege
/// model and return a pool connected as that caller. Mirrors CNPG's
/// `<svc>_owner`: `LOGIN CREATEROLE NOSUPERUSER`, nothing else.
///
/// The downstream tests run *through* this pool so they exercise the
/// same code path as production. Calling `ensure_app_role` /
/// `grant_app_access` through a SUPERUSER admin pool would hide
/// permission-related regressions — see issue #13.
pub async fn setup_caller(admin: &PgPool, admin_url: &str) -> (PgPool, String) {
    let caller = unique_role_name();
    let password = "caller_pw_for_e2e_only";

    // caller is a freshly generated unique_role_name() — matches the
    // [a-z][a-z0-9_]* validator, so identifier and literal interpolation
    // are both safe. Password is a test-only constant, no secret.
    let create_sql = format!(
        "CREATE ROLE \"{caller}\" LOGIN CREATEROLE NOSUPERUSER \
         PASSWORD '{password}'"
    );
    sqlx::query(&create_sql)
        .execute(admin)
        .await
        .expect("create caller role");

    // CNPG provisions the owner as the database owner, which carries
    // CREATE ON SCHEMA public implicitly. PG 15+ revoked the default
    // CREATE-on-public grant from the PUBLIC role, so a non-superuser
    // caller created by `CREATE ROLE` alone cannot `CREATE TABLE` in
    // public until granted. Grant it here so the tests mirror prod
    // (rather than running into "permission denied for schema public"
    // every time the caller tries to set up a table).
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

/// Open a pool as `role` against the same cluster as `admin_url`.
/// Used to switch from the "owner" caller to the application role for
/// grant/rls verification.
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
