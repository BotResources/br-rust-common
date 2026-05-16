use thiserror::Error;

/// Errors raised by the Postgres helpers.
#[derive(Debug, Error)]
pub enum PostgresError {
    /// The database URL failed TLS validation or was malformed.
    #[error("config error: {0}")]
    Config(String),

    /// A role name failed Rust-side validation before being interpolated into DDL.
    ///
    /// `ensure_app_role` requires names to match `^[a-z][a-z0-9_]*$` and to be
    /// at most 63 bytes (the Postgres identifier limit). The name goes into
    /// `format!` for the DDL so this check is the only barrier against SQL
    /// injection through the role identifier.
    #[error("invalid role name: {0:?}")]
    InvalidRoleName(String),

    /// Wrapped `sqlx::Error` from the driver.
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}
