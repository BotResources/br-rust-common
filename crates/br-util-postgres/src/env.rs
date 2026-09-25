//! The names of the variables this crate reads on its own. Part of the ops
//! contract: the `br-common-service` chart renders them and is gated against
//! these constants.
//!
//! The runtime DSN, `DATABASE_URL`, is not among them: the service reads it
//! (`br_util_boot::env::DATABASE_URL`) and hands it to
//! [`init_pool`](crate::init_pool); [`init_migration_pool`](crate::init_migration_pool)
//! falls back to it through that same constant.

/// The DSN of the database owner role, read by
/// [`init_migration_pool`](crate::init_migration_pool) to migrate at boot.
/// The owner bypasses row-level security, so this DSN never backs a request.
/// When it is not set, the migration pool falls back to `DATABASE_URL`.
pub const DATABASE_URL_OWNER: &str = "DATABASE_URL_OWNER";

/// Comma-separated bare hostnames on a trusted network segment, which a DSN
/// may reach without TLS: exact match, no port, no wildcard. Loopback is always
/// trusted. Read by [`validate_database_tls`](crate::validate_database_tls),
/// hence by both pools.
pub const TRUSTED_NETWORK_HOSTS: &str = "TRUSTED_NETWORK_HOSTS";
