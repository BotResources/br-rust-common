use thiserror::Error;

#[derive(Debug, Error)]
pub enum PostgresError {
    #[error("config error: {0}")]
    Config(String),

    #[error("invalid role name: {0:?}")]
    InvalidRoleName(String),

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
}
