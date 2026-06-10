use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};

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
///
/// Deliberately hand-rolled — **do not** replace this with
/// `PgConnectOptions::from_str(url).get_host()`. sqlx defaults an absent or
/// unparseable host to `"localhost"`, which loopback-short-circuits as
/// trusted and skips the TLS requirement entirely: a malformed URL would then
/// fail *open* to a plaintext "loopback" connection. That is exactly the
/// fail-open pattern this validator exists to prevent, so host extraction
/// stays independent of sqlx and fails closed to `""`. (sslmode parsing,
/// where sqlx has no such permissive default, *is* delegated to sqlx — see
/// [`parse_sslmode`].)
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

/// Resolve the effective `sslmode` of a PostgreSQL URL via sqlx itself.
///
/// Delegating to `PgConnectOptions::from_str(url).get_ssl_mode()` makes sqlx
/// the single source of truth for sslmode resolution — the `sslmode`/`ssl-mode`
/// aliasing, the case-insensitive value, the last-occurrence-wins on duplicates
/// and the `prefer` default are all sqlx's own behavior, so this validator can
/// never drift from what the build will actually negotiate on a sqlx bump.
///
/// A URL sqlx cannot parse (including an unknown sslmode value such as
/// `sslmode=bogus`, which `PgSslMode::from_str` rejects) is a configuration
/// error: it returns `Err`, which the caller turns into a rejection. Unlike
/// host extraction, sqlx has no permissive default for an *invalid* sslmode,
/// so deferring here is safe — it fails closed.
fn parse_sslmode(url: &str) -> Result<PgSslMode, PostgresError> {
    PgConnectOptions::from_str(url)
        .map(|opts| opts.get_ssl_mode())
        .map_err(|e| {
            PostgresError::Config(format!(
                "could not parse DATABASE_URL for TLS validation: {e}"
            ))
        })
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
    // sqlx (libpq-compatible) lets a `host=` / `hostaddr=` query parameter
    // OVERRIDE the authority host. This validator judges the authority host,
    // so an override would make it vouch for a host sqlx never connects to:
    // `postgres://localhost/db?host=remote` would pass as loopback while
    // actually dialing `remote` in plaintext. Fail closed BEFORE the loopback
    // short-circuit: the real host must live in the URL authority.
    // (`query_pairs()` percent-decodes keys, matching sqlx's own parsing.)
    if let Ok(parsed) = url::Url::parse(url)
        && parsed
            .query_pairs()
            .any(|(k, _)| k == "host" || k == "hostaddr")
    {
        return Err(PostgresError::Config(
            "DATABASE_URL overrides the target host via a host=/hostaddr= query \
             parameter; TLS validation cannot vouch for the real target — put \
             the host in the URL authority"
                .to_string(),
        ));
    }

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

    // For a genuinely remote, untrusted host the URL must parse and request a
    // TLS-enforcing sslmode. A malformed URL is a hard config error (fail
    // closed) rather than a silent pass.
    let has_tls = matches!(
        parse_sslmode(url)?,
        PgSslMode::Require | PgSslMode::VerifyCa | PgSslMode::VerifyFull
    );

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
///
/// **`Ok` does not prove the database is reachable.** `min_connections` is
/// filled *lazily* by sqlx, so this returns `Ok` as soon as the options are
/// valid; a wrong host, down server, or bad credentials surface only on the
/// **first acquire/query**, not here. To honor the fail-loud invariant
/// ("declared infra is assumed to exist; if it doesn't, fail loud / readiness
/// DOWN"), a caller must probe explicitly after init — run a `SELECT 1` and
/// only then flip its [`br_util_axum_readiness`] handle to ready. See the
/// "Wiring readiness" recipe in the crate README.
///
/// [`br_util_axum_readiness`]: https://github.com/BotResources/br-rust-common
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
///
/// Like [`init_pool`], `Ok` here does **not** prove the database is reachable —
/// sqlx fills connections lazily. In practice the very next step
/// (`ensure_app_role` / `sqlx::migrate!`) acquires a connection and so fails
/// loud immediately, but do not treat the `Ok` itself as a connectivity check.
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

    // ─── parse_sslmode (delegates to sqlx) ───────────
    //
    // These assert the *resolution* — aliasing, case, last-wins, default,
    // malformed — through `validate_database_tls`, which is what actually
    // matters. Because resolution is now sqlx's `PgConnectOptions`, these
    // prove agreement with sqlx by construction: if a sqlx bump changed any
    // of these behaviors, the assertion (and the connection) would change in
    // lockstep, with no separate copy to drift.

    #[test]
    fn require_sslmode_passes_tls_validation() {
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
    fn sslmode_among_other_params_is_honored() {
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?connect_timeout=10&sslmode=verify-full&application_name=test",
                Environment::Prod,
                false
            )
            .is_ok()
        );
    }

    #[test]
    fn absent_sslmode_defaults_to_prefer_and_rejects() {
        // sqlx defaults a missing sslmode to `prefer` (not a TLS-enforcing
        // mode), so a remote host without sslmode is rejected.
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?connect_timeout=10",
                Environment::Prod,
                false
            )
            .is_err()
        );
        assert!(
            validate_database_tls("postgres://db.example.com/db", Environment::Prod, false)
                .is_err()
        );
    }

    #[test]
    fn malformed_url_fails_closed() {
        // A remote-looking URL sqlx cannot parse is a hard config error, not
        // a silent pass — fail closed.
        assert!(matches!(
            validate_database_tls(
                "postgres://db.example.com/db?sslmode=not-a-mode",
                Environment::Prod,
                false
            ),
            Err(PostgresError::Config(_))
        ));
    }

    // ─── host=/hostaddr= query overrides (validator/connector divergence) ──

    // sqlx lets `?host=` override the authority host. The validator judges
    // the authority, so the override MUST be rejected — otherwise
    // `postgres://localhost/db?host=remote` passes as loopback while sqlx
    // dials `remote` in plaintext.
    #[test]
    fn loopback_authority_with_host_override_is_rejected() {
        assert!(matches!(
            validate_database_tls(
                "postgres://localhost/db?host=evil.example.com",
                Environment::Prod,
                false
            ),
            Err(PostgresError::Config(_))
        ));
    }

    #[test]
    fn loopback_authority_with_hostaddr_override_is_rejected() {
        assert!(matches!(
            validate_database_tls(
                "postgres://127.0.0.1/db?hostaddr=8.8.8.8",
                Environment::Prod,
                false
            ),
            Err(PostgresError::Config(_))
        ));
    }

    // `query_pairs()` percent-decodes keys exactly like sqlx's URL parsing,
    // so an encoded `%68ost=` cannot sneak past the guard.
    #[test]
    fn percent_encoded_host_override_is_rejected() {
        assert!(matches!(
            validate_database_tls(
                "postgres://localhost/db?%68ost=evil.example.com",
                Environment::Prod,
                false
            ),
            Err(PostgresError::Config(_))
        ));
    }

    // Harmless query parameters do not trip the guard.
    #[test]
    fn other_query_params_do_not_trip_the_override_guard() {
        assert!(
            validate_database_tls(
                "postgres://localhost/db?application_name=svc",
                Environment::Prod,
                false
            )
            .is_ok()
        );
    }

    // ─── P1: last sslmode wins (sqlx behavior) ──

    #[test]
    fn duplicate_sslmode_require_then_disable_rejects_tls() {
        // sqlx reassigns on each occurrence, so the last value (disable) wins.
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?sslmode=require&sslmode=disable",
                Environment::Prod,
                false
            )
            .is_err()
        );
    }

    // ─── P2: ssl-mode alias + case insensitive (sqlx behavior) ──

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
