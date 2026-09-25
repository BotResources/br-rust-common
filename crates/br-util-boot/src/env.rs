//! The names of the variables a service reads at boot through [`BootEnv`].
//!
//! These constants are the single definition of each name: a service reads the
//! variable through [`BootEnv`] (or, when it reads one on its own, through the
//! constant — never through a literal), and the `br-common-service` Helm chart
//! is gated against them, so the chart cannot render a name the code does not
//! read. The Postgres names that only `br-util-postgres` reads
//! (`DATABASE_URL_OWNER`, `TRUSTED_NETWORK_HOSTS`) are that crate's.
//!
//! [`BootEnv`]: crate::BootEnv

/// The logical environment the process runs in: `local`, `dev`, `test`, `uat`
/// or `prod` ([`Environment`](crate::Environment)). Required.
pub const ENVIRONMENT: &str = "ENVIRONMENT";

/// The TCP port the HTTP listener binds, 1 to 65535. Required: in Kubernetes it
/// is also the container port and the Service port, so a default in the binary
/// could only hide a chart that forgot it.
pub const PORT: &str = "PORT";

/// The IP address the HTTP listener binds. Optional: when it is not set, the
/// listener binds every IPv4 interface (`0.0.0.0`), which is what a pod needs
/// for its probes to reach it.
pub const HOST: &str = "HOST";

/// The Postgres DSN of the service's least-privilege runtime role. Required.
/// It carries a password: it is never logged ([`DatabaseUrl`](crate::DatabaseUrl)
/// redacts it).
pub const DATABASE_URL: &str = "DATABASE_URL";

/// The URL of the NATS server. Required. It may carry credentials: it is never
/// logged ([`NatsUrl`](crate::NatsUrl) redacts them).
pub const NATS_URL: &str = "NATS_URL";
