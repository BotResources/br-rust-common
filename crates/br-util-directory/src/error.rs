use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DirectoryError {
    #[error("directory KV error: {0}")]
    Kv(String),

    #[error("directory wire (de)serialization error: {0}")]
    Wire(String),

    #[cfg(feature = "consumer")]
    #[error("directory projection persistence error: {0}")]
    Persistence(#[from] sqlx::Error),

    #[cfg(feature = "consumer")]
    #[error("directory migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[cfg(feature = "consumer")]
    #[error("directory pool initialization error: {0}")]
    Pool(String),
}
