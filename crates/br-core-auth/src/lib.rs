//! Passport identity DTO and `X-Passport` header codec.
//!
//! `Passport` is the cross-service identity representation built by
//! `svc-identity` and consumed by every downstream service. The
//! `PassportHeader` trait encapsulates the base64/JSON encoding used for
//! the `X-Passport` HTTP header.

mod auth_method;
mod error;
mod header;
mod passport;

pub use auth_method::AuthMethod;
pub use error::PassportError;
pub use header::PassportHeader;
pub use passport::Passport;
