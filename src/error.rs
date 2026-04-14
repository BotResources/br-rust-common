use thiserror::Error;

#[derive(Debug, Error)]
pub enum InfraError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("unauthenticated: {0}")]
    Unauthenticated(String),
}
