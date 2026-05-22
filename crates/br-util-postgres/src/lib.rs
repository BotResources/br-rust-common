//! Postgres helpers shared across BotResources services.
//!
//! - [`init_pool`] / [`init_migration_pool`] — long-lived app pool + short-lived migration pool
//! - [`validate_database_tls`] — mirrors sqlx sslmode parsing and enforces TLS for remote hosts
//! - [`ensure_app_role`] — idempotent CREATE ROLE + ALTER PASSWORD for the two-role model
//! - [`set_rls_context`] — transaction-local `set_config(..., true)` for RLS identity
//! - [`grant_app_access`] — post-migration GRANTs for the app role
//!
//! The [`Environment`] enum is a minimal carrier for the "is this prod?" check
//! used by TLS validation. A richer config crate may replace it later.

mod error;
mod grant;
mod net;
mod pool;
mod rls;
mod role;

#[cfg(test)]
mod test_support;

pub use error::PostgresError;
pub use grant::grant_app_access;
pub use pool::{Environment, init_migration_pool, init_pool, validate_database_tls};
pub use rls::set_rls_context;
pub use role::ensure_app_role;
