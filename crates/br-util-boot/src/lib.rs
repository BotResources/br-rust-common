//! The boot environment of a BotResources service.
//!
//! [`env`](mod@env) names the variables a service reads at boot, once, for every
//! service and for the `br-common-service` Helm chart, which is gated against
//! these constants. [`BootEnv`] reads them, typed and validated, and reports
//! every problem at once.
//!
//! ```
//! use std::collections::HashMap;
//! use std::env::VarError;
//!
//! use br_util_boot::{BootEnv, Environment, env};
//!
//! let vars = HashMap::from([
//!     (env::ENVIRONMENT, "dev"),
//!     (env::PORT, "8004"),
//!     (env::DATABASE_URL, "postgres://app@pg-rw:5432/charter"),
//!     (env::NATS_URL, "nats://nats:4222"),
//! ]);
//! // A binary calls `BootEnv::from_env()`; a test supplies the variables.
//! let boot = BootEnv::from_lookup(|name| {
//!     vars.get(name).map(|v| v.to_string()).ok_or(VarError::NotPresent)
//! })?;
//!
//! assert_eq!(boot.environment, Environment::Dev);
//! assert_eq!(boot.listen_addr().to_string(), "0.0.0.0:8004");
//! # Ok::<(), br_util_boot::BootEnvError>(())
//! ```

mod boot_env;
mod database_url;
pub mod env;
mod environment;
mod error;
mod nats_url;

pub use boot_env::{BootEnv, DEFAULT_HOST};
pub use database_url::DatabaseUrl;
pub use environment::Environment;
pub use error::{BootEnvError, InvalidValue, VarProblem};
pub use nats_url::NatsUrl;
