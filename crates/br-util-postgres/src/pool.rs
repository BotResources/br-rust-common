use std::str::FromStr;
use std::time::Duration;

use sqlx::postgres::{PgConnectOptions, PgPool, PgPoolOptions, PgSslMode};

use crate::error::PostgresError;
use crate::net::{is_loopback, is_on_trusted_network, resolve_trusted_network_hosts};

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

fn parse_sslmode(url: &str) -> Result<PgSslMode, PostgresError> {
    PgConnectOptions::from_str(url)
        .map(|opts| opts.get_ssl_mode())
        .map_err(|e| {
            PostgresError::Config(format!(
                "could not parse DATABASE_URL for TLS validation: {e}"
            ))
        })
}

pub fn validate_database_tls(url: &str) -> Result<(), PostgresError> {
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

    if is_loopback(&host) {
        return Ok(());
    }

    if is_on_trusted_network(&host, &resolve_trusted_network_hosts()) {
        return Ok(());
    }

    let has_tls = matches!(
        parse_sslmode(url)?,
        PgSslMode::Require | PgSslMode::VerifyCa | PgSslMode::VerifyFull
    );

    if !has_tls {
        return Err(PostgresError::Config(format!(
            "remote database connection to '{host}' requires TLS: \
             add sslmode=require (or verify-ca/verify-full) to DATABASE_URL, \
             or declare the host in TRUSTED_NETWORK_HOSTS if it sits on a \
             trusted network segment"
        )));
    }

    Ok(())
}

pub async fn init_pool(database_url: &str) -> Result<PgPool, PostgresError> {
    validate_database_tls(database_url)?;

    PgPoolOptions::new()
        .max_connections(20)
        .min_connections(2)
        .acquire_timeout(Duration::from_secs(8))
        .connect(database_url)
        .await
        .map_err(PostgresError::Db)
}

pub async fn init_migration_pool() -> Result<PgPool, PostgresError> {
    let url = std::env::var("DATABASE_URL_OWNER")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .map_err(|_| {
            PostgresError::Config(
                "DATABASE_URL_OWNER or DATABASE_URL must be set for migrations".to_string(),
            )
        })?;
    validate_database_tls(&url)?;

    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .map_err(PostgresError::Db)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn localhost_always_allowed() {
        assert!(validate_database_tls("postgres://localhost/db").is_ok());
    }

    #[test]
    fn ipv6_loopback_always_allowed() {
        assert!(validate_database_tls("postgres://user@[::1]:5432/db").is_ok());
    }

    #[test]
    fn unparseable_host_fails_closed() {
        assert!(validate_database_tls("postgres://").is_err());
        assert_eq!(extract_pg_host("postgres://"), "");
    }

    #[test]
    fn remote_without_tls_rejected() {
        assert!(validate_database_tls("postgres://db.example.com/db").is_err());
    }

    #[test]
    fn remote_with_tls_accepted() {
        assert!(validate_database_tls("postgres://db.example.com/db?sslmode=require").is_ok());
    }

    #[test]
    fn remote_without_tls_rejected_regardless_of_sslmode_off() {
        assert!(validate_database_tls("postgres://db.example.com/db?sslmode=disable").is_err());
    }

    #[test]
    fn require_sslmode_passes_tls_validation() {
        assert!(validate_database_tls("postgres://db.example.com/db?sslmode=require").is_ok());
    }

    #[test]
    fn sslmode_among_other_params_is_honored() {
        assert!(
            validate_database_tls(
                "postgres://db.example.com/db?connect_timeout=10&sslmode=verify-full&application_name=test",
            )
            .is_ok()
        );
    }

    #[test]
    fn absent_sslmode_defaults_to_prefer_and_rejects() {
        assert!(validate_database_tls("postgres://db.example.com/db?connect_timeout=10").is_err());
        assert!(validate_database_tls("postgres://db.example.com/db").is_err());
    }

    #[test]
    fn malformed_url_fails_closed() {
        assert!(matches!(
            validate_database_tls("postgres://db.example.com/db?sslmode=not-a-mode"),
            Err(PostgresError::Config(_))
        ));
    }

    #[test]
    fn loopback_authority_with_host_override_is_rejected() {
        assert!(matches!(
            validate_database_tls("postgres://localhost/db?host=evil.example.com"),
            Err(PostgresError::Config(_))
        ));
    }

    #[test]
    fn loopback_authority_with_hostaddr_override_is_rejected() {
        assert!(matches!(
            validate_database_tls("postgres://127.0.0.1/db?hostaddr=8.8.8.8"),
            Err(PostgresError::Config(_))
        ));
    }

    #[test]
    fn percent_encoded_host_override_is_rejected() {
        assert!(matches!(
            validate_database_tls("postgres://localhost/db?%68ost=evil.example.com"),
            Err(PostgresError::Config(_))
        ));
    }

    #[test]
    fn other_query_params_do_not_trip_the_override_guard() {
        assert!(validate_database_tls("postgres://localhost/db?application_name=svc").is_ok());
    }

    #[test]
    fn duplicate_sslmode_require_then_disable_rejects_tls() {
        assert!(
            validate_database_tls("postgres://db.example.com/db?sslmode=require&sslmode=disable")
                .is_err()
        );
    }

    #[test]
    fn ssl_mode_hyphenated_passes_tls_validation() {
        assert!(validate_database_tls("postgres://db.example.com/db?ssl-mode=verify-full").is_ok());
    }

    #[test]
    fn uppercase_sslmode_passes_tls_validation() {
        assert!(validate_database_tls("postgres://db.example.com/db?sslmode=REQUIRE").is_ok());
    }

    #[test]
    fn mixed_case_ssl_mode_passes_tls_validation() {
        assert!(validate_database_tls("postgres://db.example.com/db?ssl-mode=Verify-Ca").is_ok());
    }
}

#[cfg(test)]
mod live_tls_tests {
    use br_test_support::{test_db_url, test_tls_db_url};
    use sqlx::Connection;
    use sqlx::postgres::PgConnection;

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
