//! Postgres helpers shared across BotResources services.
//!
//! - [`init_pool`] / [`init_migration_pool`] — long-lived app pool + short-lived migration pool
//! - [`validate_database_tls`] — sslmode resolved by sqlx itself; independent fail-closed
//!   host judgment (rejects `host=`/`hostaddr=` overrides); requires TLS for every
//!   remote host not on the trusted-network list, with no escape hatch
//! - [`ensure_app_role`] — idempotent CREATE ROLE + ALTER PASSWORD for the two-role model
//! - [`set_rls_context`] — transaction-local `set_config(..., true)` for RLS identity
//! - [`grant_app_access`] — post-migration GRANTs for the app role
//!
//! TLS validation requires a TLS-enforcing `sslmode` for every remote host that
//! is not on the trusted-network list — there is no environment-gated escape
//! hatch. A host reached over plaintext (e.g. an intra-namespace CloudNativePG
//! database) must be declared in `TRUSTED_NETWORK_HOSTS`.

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
pub use pool::{init_migration_pool, init_pool, validate_database_tls};
pub use rls::set_rls_context;
pub use role::ensure_app_role;
