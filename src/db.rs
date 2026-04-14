use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::config::Environment;
use crate::error::InfraError;
use crate::net::is_localhost;

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
) -> Result<(), InfraError> {
    let host = extract_pg_host(url);

    if is_localhost(&host) {
        return Ok(());
    }

    let has_tls = url.contains("sslmode=require")
        || url.contains("sslmode=verify-ca")
        || url.contains("sslmode=verify-full");

    if !has_tls {
        if allow_insecure && environment != Environment::Prod {
            tracing::warn!(
                host = %host,
                "remote database connection without TLS — allowed by ALLOW_INSECURE"
            );
            return Ok(());
        }

        return Err(InfraError::Config(format!(
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
) -> Result<PgPool, InfraError> {
    validate_database_tls(database_url, environment, allow_insecure)?;

    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(8))
        .connect(database_url)
        .await
        .map_err(InfraError::Db)
}

/// Short-lived pool for running migrations (owner role).
///
/// Reads `DATABASE_URL_OWNER` (falls back to `DATABASE_URL`).
/// Use this to run migrations, then drop it before creating the app pool.
pub async fn init_migration_pool(
    environment: Environment,
    allow_insecure: bool,
) -> Result<PgPool, InfraError> {
    let url = std::env::var("DATABASE_URL_OWNER")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| {
            InfraError::Config(
                "DATABASE_URL_OWNER or DATABASE_URL must be set for migrations".to_string(),
            )
        })?;
    validate_database_tls(&url, environment, allow_insecure)?;

    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .map_err(InfraError::Db)
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
        assert!(validate_database_tls(
            "postgres://localhost/db",
            Environment::Prod,
            false
        )
        .is_ok());
    }

    #[test]
    fn remote_without_tls_rejected_in_prod() {
        assert!(validate_database_tls(
            "postgres://db.example.com/db",
            Environment::Prod,
            false
        )
        .is_err());
    }

    #[test]
    fn remote_with_tls_accepted() {
        assert!(validate_database_tls(
            "postgres://db.example.com/db?sslmode=require",
            Environment::Prod,
            false
        )
        .is_ok());
    }

    #[test]
    fn remote_without_tls_allowed_by_insecure_in_dev() {
        assert!(validate_database_tls(
            "postgres://db.example.com/db",
            Environment::Dev,
            true
        )
        .is_ok());
    }

    #[test]
    fn remote_without_tls_rejected_in_prod_even_with_insecure() {
        assert!(validate_database_tls(
            "postgres://db.example.com/db",
            Environment::Prod,
            true
        )
        .is_err());
    }
}
