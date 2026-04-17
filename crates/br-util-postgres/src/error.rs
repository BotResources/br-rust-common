use thiserror::Error;

/// Errors raised by the Postgres helpers.
#[derive(Debug, Error)]
pub enum PostgresError {
    /// The database URL failed TLS validation or was malformed.
    #[error("config error: {0}")]
    Config(String),

    /// Wrapped `sqlx::Error` from the driver.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}
