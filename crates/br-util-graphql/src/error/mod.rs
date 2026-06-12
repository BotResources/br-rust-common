//! The 3-layer edge error mapping: **domain error → [`EdgeError`] → transport**.
//!
//! - [`ErrorCode`] — the canonical, cross-service code set the frontends bind
//!   to (`code.rs`). The published contract.
//! - [`EdgeError`] — the application-layer error a service builds from its own
//!   domain error, carrying the class code + an optional precise `reason_code`
//!   + params + a never-returned internal detail (`edge.rs`).
//! - The two render edges: `into_gql` → an `async_graphql::Error` with the
//!   contract extensions (`gql.rs`), and `IntoResponse` → an Axum HTTP response
//!   with the mirrored JSON body (`rest.rs`).
//!
//! A consuming service implements one `From<MyDomainError> for EdgeError`, then
//! every resolver / handler returns `Result<_, EdgeError>` and the transport is
//! uniform.

mod code;
mod edge;
mod gql;
mod rest;

pub use code::ErrorCode;
pub use edge::EdgeError;
