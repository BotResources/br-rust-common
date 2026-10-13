use std::time::Duration;

use sqlx::postgres::{PgPool, PgPoolOptions};

use br_util_boot::env::DATABASE_URL;

use crate::env::DATABASE_URL_OWNER;
use crate::error::PostgresError;
use crate::tls::validate_database_tls;

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
    let url = std::env::var(DATABASE_URL_OWNER)
        .or_else(|_| std::env::var(DATABASE_URL))
        .map_err(|_| {
            PostgresError::Config(format!(
                "{DATABASE_URL_OWNER} or {DATABASE_URL} must be set for migrations"
            ))
        })?;
    validate_database_tls(&url)?;

    PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .map_err(PostgresError::Db)
}

#[cfg(test)]
mod live_tls_tests {
    use br_test_support::{require_test_db_url, require_test_tls_db_url};
    use sqlx::Connection;
    use sqlx::postgres::PgConnection;

    fn with_param(url: &str, param: &str) -> String {
        let sep = if url.contains('?') { '&' } else { '?' };
        format!("{url}{sep}{param}")
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a (plaintext) PG 16+"]
    async fn backend_is_compiled_in() {
        let url = require_test_db_url();
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
        let url = require_test_tls_db_url();
        let tls_url = with_param(&url, "sslmode=require");

        PgConnection::connect(&tls_url)
            .await
            .expect("TLS handshake against a TLS-enabled server must succeed")
            .close()
            .await
            .ok();
    }
}
