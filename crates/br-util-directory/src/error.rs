use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum DirectoryError {
    #[error("directory fabric error: {0}")]
    Fabric(#[from] br_util_nats_fabric::FabricError),

    #[error("directory kv key error: {0}")]
    KvKey(#[from] br_util_nats_fabric::KvKeyError),

    #[error("directory roster is unavailable: the identity manifest is absent")]
    ManifestAbsent,

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

#[cfg(feature = "consumer")]
impl From<br_util_nats_fabric::ProjectionError<DirectoryError>> for DirectoryError {
    fn from(err: br_util_nats_fabric::ProjectionError<DirectoryError>) -> Self {
        match err {
            br_util_nats_fabric::ProjectionError::Fabric(e) => DirectoryError::Fabric(e),
            br_util_nats_fabric::ProjectionError::Sink(e) => e,
        }
    }
}
