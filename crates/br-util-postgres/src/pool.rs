use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

use crate::error::PostgresError;
use crate::net::{is_loopback, is_on_trusted_network, resolve_trusted_network_hosts};

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
/// An unparseable host yields `""`, which is on no trusted list, so TLS
/// validation **fails closed** (TLS required) rather than skipping the check.
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
            .unwrap_or_default()
            .to_string();
    }

    host_port.split(':').next().unwrap_or_default().to_string()
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
/// - Hosts on a trusted network segment are always allowed: loopback, plus
///   any host listed in `TRUSTED_NETWORK_HOSTS` (e.g. an intra-namespace
///   CloudNativePG database reached over plaintext behind a default-deny
///   `NetworkPolicy`). We trust the network segment, not the host.
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

    // Loopback short-circuits before any env read, so a loopback host never
    // triggers the `TRUSTED_HOSTS` deprecation warning. Only a genuinely
    // remote host pays the cost of resolving the trusted-network list.
    if is_loopback(&host) {
        return Ok(());
    }

    if is_on_trusted_network(&host, &resolve_trusted_network_hosts()) {
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
        // Loopback short-circuits before any env read, so this is Ok regardless
        // of TLS params and never emits the deprecation warning.
        assert!(validate_database_tls("postgres://localhost/db", Environment::Prod, false).is_ok());
    }

    #[test]
    fn ipv6_loopback_always_allowed() {
        assert!(
            validate_database_tls("postgres://user@[::1]:5432/db", Environment::Prod, false)
                .is_ok()
        );
    }

    #[test]
    fn unparseable_host_fails_closed() {
        // A URL with no host parses to "" — on no trusted list, no loopback —
        // so TLS is required and the connection is rejected (fail closed).
        assert!(validate_database_tls("postgres://", Environment::Prod, false).is_err());
        assert_eq!(extract_pg_host("postgres://"), "");
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

/// Live-Postgres tests that prove a TLS backend is actually compiled into
/// sqlx — the defect this crate's 0.6.1 fix closed.
///
/// Until 0.6.1 the workspace declared `sqlx` with `runtime-tokio` but **no**
/// TLS feature. With no backend, sqlx 0.8 fails *before any network I/O* on a
/// `sslmode=require` URL with the client-side error "TLS upgrade required by
/// connect options but SQLx was built without TLS support enabled"
/// (`sqlx-core/src/net/tls/mod.rs::error_if_unavailable`). So
/// `validate_database_tls` could pass a URL that the build could never honor.
///
/// `backend_is_compiled_in` makes the difference observable **without a TLS
/// server**: pointed at the ordinary *plaintext* `TEST_DATABASE_URL` with
/// `sslmode=require` appended, a TLS-less build fails client-side ("built
/// without TLS support") whereas a TLS-enabled build gets past that gate,
/// sends the `SSLRequest`, the plaintext server answers `N`, and sqlx fails
/// with the *server-side* error "server does not support TLS"
/// (`sqlx-postgres/src/connection/tls.rs`). Asserting the message is the
/// server-side one — and explicitly **not** the client-side one — proves the
/// backend is linked in.
///
/// `full_handshake_succeeds` is the positive path: gated on a separate
/// `TEST_TLS_DATABASE_URL` (a server started with `ssl=on` and a cert), it
/// connects with `sslmode=require` and expects success. It skips silently
/// when that var is unset — TLS-server provisioning is heavier than a plain
/// `postgres:16-alpine`, so this stays opt-in (wired in CI by the
/// `e2e-postgres-tls` job).
#[cfg(test)]
mod live_tls_tests {
    use crate::test_support::{test_db_url, test_tls_db_url};
    use sqlx::Connection;
    use sqlx::postgres::PgConnection;

    /// Append a query parameter to a Postgres URL, handling the `?`/`&` join.
    fn with_param(url: &str, param: &str) -> String {
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}{param}")
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a (plaintext) PG 16+"]
    async fn backend_is_compiled_in() {
        let Some(url) = test_db_url() else { return };
        let tls_url = with_param(&url, "sslmode=require");

        let err = PgConnection::connect(&tls_url)
            .await
            .expect_err("plaintext server must refuse a required-TLS connection");

        // The whole point: the failure must be the *server-side* refusal
        // (backend present, handshake attempted, server said N), NOT the
        // *client-side* "no backend" error. If the TLS feature regresses out
        // of the sqlx dependency, this assertion flips and the test fails.
        let msg = err.to_string();
        assert!(
            msg.contains("server does not support TLS"),
            "expected the server-side TLS refusal, got: {msg}"
        );
        assert!(
            !msg.contains("built without TLS support"),
            "sqlx was built WITHOUT a TLS backend — the rustls feature is \
             missing from the workspace sqlx dependency. Got: {msg}"
        );
        assert!(
            matches!(err, sqlx::Error::Tls(_)),
            "expected sqlx::Error::Tls, got: {err:?}"
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_TLS_DATABASE_URL pointing at a TLS-enabled PG 16+"]
    async fn full_handshake_succeeds() {
        let Some(url) = test_tls_db_url() else { return };
        let tls_url = with_param(&url, "sslmode=require");

        PgConnection::connect(&tls_url)
            .await
            .expect("TLS handshake against a TLS-enabled server must succeed")
            .close()
            .await
            .ok();
    }
}
