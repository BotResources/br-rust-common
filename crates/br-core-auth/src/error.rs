use thiserror::Error;

#[derive(Debug, Error)]
pub enum PassportError {
    #[error("malformed passport: {0}")]
    Malformed(String),
}
