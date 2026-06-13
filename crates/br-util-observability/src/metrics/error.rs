use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum MetricsError {
    #[error("invalid_buckets detail={0}")]
    Buckets(String),
    #[error("recorder_install_failed detail={0}")]
    Install(String),
}
