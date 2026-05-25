//! Passport identity DTO and `X-Passport` header codec.
//!
//! `Passport` is the cross-service identity representation built by
//! `svc-identity` and consumed by every downstream service. The
//! `PassportHeader` trait encapsulates the base64/JSON encoding used for
//! the `X-Passport` HTTP header.
//!
//! For PAT authentication, [`bearer_token_key`] and [`BearerTokenEntry`]
//! define the canonical key/value contract for the `bearer_tokens` NATS KV
//! bucket — the shared store every service hashes against to resolve a PAT.

mod auth_method;
mod bearer_token;
mod error;
mod header;
mod passport;
mod session_cookie;

pub use auth_method::AuthMethod;
pub use bearer_token::{BearerTokenEntry, bearer_token_key};
pub use error::PassportError;
pub use header::PassportHeader;
pub use passport::Passport;
pub use session_cookie::{extract_session_id, session_cookie_name};
