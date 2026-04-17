use thiserror::Error;

/// Errors raised when decoding or validating a `Passport`.
#[derive(Debug, Error)]
pub enum PassportError {
    /// The `X-Passport` header could not be decoded or deserialized.
    #[error("malformed passport: {0}")]
    Malformed(String),
}
