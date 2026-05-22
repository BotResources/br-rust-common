use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::error::PostgresError;
use crate::net::is_localhost;

/// Deployment environment flag used by TLS validation.
///
/// Only `Prod` is load-bearing today (it forbids the `allow_insecure` bypass).
/// The other variants exist to preserve naming across services.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Local,
    Dev,
    Test,
    Prod,
}

// ---------------------------------------------------------------------------
// TLS validation helpers
// ---------------------------------------------------------------------------

/// Extract the host from a PostgreSQL connection URL.
///
/// Handles `postgres://[user[:pass]@]host[:port]/db[?params]`.
/// Returns `"localhost"` if parsing fails.
fn extract_pg_host(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("postgres://")
        .or_else(|| url.strip_prefix("postgresql://"))
        .unwrap_or(url);

    let after_auth = match without_scheme.find('@') {
        Some(pos) => &without_scheme[pos + 1..],
        None => without_scheme,
    };

    let host_port = after_auth.split('/').next().unwrap_or(after_auth);
    let host_port = host_port.split('?').next().unwrap_or(host_port);

    if host_port.starts_with('[') {
        return host_port
            .trim_start_matches('[')
            .split(']')
            .next()
            .unwrap_or("localhost")
            .to_string();
    }

    host_port
        .split(':')
        .next()
        .unwrap_or("localhost")
        .to_string()
}

/// Extract the `sslmode` query parameter value from a PostgreSQL URL.
///
/// Matches sqlx behavior (sqlx-postgres 0.8.6):
/// - Accepts both `sslmode` and `ssl-mode` as key names (parse.rs:52)
/// - Lowercases the value (`PgSslMode::from_str` does `to_ascii_lowercase()`)
/// - Returns the **last** occurrence if duplicated (parse.rs:50 reassigns each match)
fn extract_sslmode(url: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    let mut result = None;
    for param in query.split('&') {
        let Some((key, value)) = param.split_once('=') else {
            continue;
        };
        if key == "sslmode" || key == "ssl-mode" {
            result = Some(value.to_ascii_lowercase());
        }
    }
    result
}

/// Validate that a remote PostgreSQL connection uses TLS.
///
/// - Localhost connections are always allowed (no TLS needed for local dev).
/// - Remote connections must include `sslmode=require`, `sslmode=verify-ca`,
///   or `sslmode=verify-full` in the URL.
/// - In non-production environments, `allow_insecure=true` bypasses this check.
/// - In production, TLS is always required for remote connections.
pub fn validate_database_tls(
    url: &str,
    environment: Environment,
    allow_insecure: bool,
) -> Result<(), PostgresError> {
    let host = extract_pg_host(url);

    if is_localhost(&host) {
        return Ok(());
    }

    let has_tls = extract_sslmode(url)
        .is_some_and(|m| matches!(m.as_str(), "require" | "verify-ca" | "verify-full"));

    if !has_tls {
        if allow_insecure && environment != Environment::Prod {
            tracing::warn!(
                host = %host,
                "remote database connection without TLS — allowed by ALLOW_INSECURE"
            );
            return Ok(());
        }

        return Err(PostgresError::Config(format!(
            "remote database connection to '{host}' requires TLS: \
             add sslmode=require (or verify-ca/verify-full) to DATABASE_URL"
        )));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Pool initialization
// ---------------------------------------------------------------------------

/// Create a PostgreSQL connection pool with sensible defaults.
///
/// Validates TLS requirements before connecting. Does NOT run migrations —
/// each service owns its own migrations.
pub async fn init_pool(
    database_url: &str,
    environment: Environment,
    allow_insecure: bool,
) -> Result<PgPool, PostgresError> {
    validate_database_tls(database_url, environment, allow_insecure)?;

    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(8))
        .connect(database_url)
        .await
        .map_err(PostgresError::Db)
}

/// Short-lived pool for running migrations (owner role).
///
/// Reads `DATABASE_URL_OWNER` (falls back to `DATABASE_URL`).
/// Use this to run migrations, then drop it before creating the app pool.
pub async fn init_migration_pool(
    environment: Environment,
    allow_insecure: bool,
) -> Result<PgPool, PostgresError> {
    let url = std::env::var("DATABASE_URL_OWNER")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| {
            PostgresError::Config(
                "DATABASE_URL_OWNER or DATABASE_URL must be set for migrations".to_string(),
            )
        })?;
    validate_database_tls(&url, environment, allow_insecure)?;

    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .map_err(PostgresError::Db)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── extract_pg_host ──────────────────────────────

    #[test]
    fn extracts_host_from_standard_url() {
        assert_eq!(
            extract_pg_host("postgres://user:pass@db.example.com:5432/mydb"),
            "db.example.com"
        );
    }

    #[test]
    fn extracts_host_without_port() {
        assert_eq!(
            extract_pg_host("postgres://user@db.example.com/mydb"),
            "db.example.com"
        );
    }

    #[test]
    fn extracts_host_without_auth() {
        assert_eq!(
            extract_pg_host("postgres://localhost:5432/mydb"),
            "localhost"
        );
    }

    #[test]
    fn extracts_ipv6_host() {
        assert_eq!(extract_pg_host("postgres://user@[::1]:5432/mydb"), "::1");
    }

    #[test]
    fn extracts_host_with_params() {
        assert_eq!(
            extract_pg_host("postgres://user@host/db?sslmode=require"),
            "host"
        );
    }

    // ─── validate_database_tls ────────────────────────

    #[test]
    fn localhost_always_allowed() {
        assert!(validate_database_tls("postgres://localhost/db", Environment::Prod, false).is_ok());
    }

    #[test]
    fn remote_without_tls_rejected_in_prod() {
        assert!(
            validate_database_tls("postgres://db.example.com/db", Environment::Prod, false)
                .is_err()
        );
    }

    #[test]
    fn remote_with_tls_accepted() {
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?sslmode=require",
                Environment::Prod,
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn remote_without_tls_allowed_by_insecure_in_dev() {
        assert!(
            validate_database_tls("postgres://db.example.com/db", Environment::Dev, true).is_ok()
        );
    }

    #[test]
    fn remote_without_tls_rejected_in_prod_even_with_insecure() {
        assert!(
            validate_database_tls("postgres://db.example.com/db", Environment::Prod, true).is_err()
        );
    }

    // ─── extract_sslmode ─────────────────────────────

    #[test]
    fn extracts_sslmode_require() {
        assert_eq!(
            extract_sslmode("postgres://host/db?sslmode=require"),
            Some("require".to_string())
        );
    }

    #[test]
    fn extracts_sslmode_among_other_params() {
        assert_eq!(
            extract_sslmode("postgres://host/db?connect_timeout=10&sslmode=verify-full&app=test"),
            Some("verify-full".to_string())
        );
    }

    #[test]
    fn returns_none_when_no_sslmode() {
        assert_eq!(
            extract_sslmode("postgres://host/db?connect_timeout=10"),
            None
        );
    }

    #[test]
    fn returns_none_when_no_query_string() {
        assert_eq!(extract_sslmode("postgres://host/db"), None);
    }

    #[test]
    fn sslmode_in_path_does_not_count_as_param() {
        // A URL where "sslmode=require" appears in a non-query-param position
        // must not be treated as having TLS enabled.
        assert!(
            validate_database_tls(
                "postgres://db.example.com/app?foo=sslmode=require",
                Environment::Prod,
                false
            )
            .is_err()
        );
    }

    // ─── P1: last sslmode wins (matches sqlx behavior) ──

    #[test]
    fn last_sslmode_wins_when_duplicated() {
        // sqlx reassigns on each occurrence, so the last value wins.
        assert_eq!(
            extract_sslmode("postgres://host/db?sslmode=require&sslmode=disable"),
            Some("disable".to_string())
        );
    }

    #[test]
    fn duplicate_sslmode_require_then_disable_rejects_tls() {
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?sslmode=require&sslmode=disable",
                Environment::Prod,
                false
            )
            .is_err()
        );
    }

    // ─── P2: ssl-mode alias + case insensitive (matches sqlx) ──

    #[test]
    fn accepts_ssl_mode_hyphenated_key() {
        assert_eq!(
            extract_sslmode("postgres://host/db?ssl-mode=require"),
            Some("require".to_string())
        );
    }

    #[test]
    fn ssl_mode_hyphenated_passes_tls_validation() {
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?ssl-mode=verify-full",
                Environment::Prod,
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn sslmode_value_is_case_insensitive() {
        assert_eq!(
            extract_sslmode("postgres://host/db?sslmode=VERIFY-FULL"),
            Some("verify-full".to_string())
        );
    }

    #[test]
    fn uppercase_sslmode_passes_tls_validation() {
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?sslmode=REQUIRE",
                Environment::Prod,
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn mixed_case_ssl_mode_passes_tls_validation() {
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?ssl-mode=Verify-Ca",
                Environment::Prod,
                false
            )
            .is_ok()
        );
    }
}
