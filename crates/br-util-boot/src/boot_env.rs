use std::env::VarError;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU16;

use crate::database_url::DatabaseUrl;
use crate::env;
use crate::environment::Environment;
use crate::error::{BootEnvError, InvalidValue, VarProblem};
use crate::nats_url::NatsUrl;

/// The listener binds every IPv4 interface when `HOST` is not set: a pod's
/// probes reach it on the pod IP, never on loopback.
pub const DEFAULT_HOST: IpAddr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);

/// The boot environment of a service, read once, typed and validated: the
/// variables the `br-common-service` chart renders for the binary (and `HOST`,
/// which it leaves to its default).
///
/// Every value is a type that cannot hold an invalid one, and the two URLs
/// redact their credentials in `Debug`, so the whole struct may be logged.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BootEnv {
    /// `ENVIRONMENT`.
    pub environment: Environment,
    /// `HOST`, [`DEFAULT_HOST`] when it is not set.
    pub host: IpAddr,
    /// `PORT`.
    pub port: NonZeroU16,
    /// `DATABASE_URL`: the runtime role's DSN.
    pub database_url: DatabaseUrl,
    /// `NATS_URL`.
    pub nats_url: NatsUrl,
}

impl BootEnv {
    /// Read the boot environment from the process environment.
    ///
    /// Every variable is read, and every problem reported at once: a missing,
    /// empty, non-UTF-8 or invalid value. Nothing is defaulted but `HOST`.
    ///
    /// This reads the environment; it never writes it. A binary that loads a
    /// `.env` file for local development either does so before any thread
    /// starts (`std::env::set_var` is unsound once they have), or reads the
    /// file into a map and uses [`from_lookup`](Self::from_lookup).
    pub fn from_env() -> Result<Self, BootEnvError> {
        Self::from_lookup(|name| std::env::var(name))
    }

    /// Read the boot environment through `lookup`, which has the shape of
    /// [`std::env::var`]: a test supplies its map, a binary with a `.env` file
    /// the process environment with that file filling in what it does not set.
    pub fn from_lookup<F>(lookup: F) -> Result<Self, BootEnvError>
    where
        F: FnMut(&str) -> Result<String, VarError>,
    {
        let mut reader = Reader {
            lookup,
            problems: Vec::new(),
        };
        let environment = reader.required(env::ENVIRONMENT, str::parse);
        let host = reader.optional(env::HOST, DEFAULT_HOST, parse_host);
        let port = reader.required(env::PORT, parse_port);
        let database_url = reader.required(env::DATABASE_URL, str::parse);
        let nats_url = reader.required(env::NATS_URL, str::parse);

        // Each read that yields `None` records exactly one problem, and one
        // that yields a value records none: all values present means no problem.
        match (environment, host, port, database_url, nats_url) {
            (Some(environment), Some(host), Some(port), Some(database_url), Some(nats_url)) => {
                Ok(Self {
                    environment,
                    host,
                    port,
                    database_url,
                    nats_url,
                })
            }
            _ => Err(BootEnvError::new(reader.problems)),
        }
    }

    /// The address the HTTP listener binds: `HOST`:`PORT`.
    pub fn listen_addr(&self) -> SocketAddr {
        SocketAddr::new(self.host, self.port.get())
    }
}

fn parse_port(raw: &str) -> Result<NonZeroU16, InvalidValue> {
    raw.parse()
        .map_err(|_| InvalidValue::new(format!("expected a TCP port from 1 to 65535, got {raw:?}")))
}

fn parse_host(raw: &str) -> Result<IpAddr, InvalidValue> {
    raw.parse().map_err(|_| {
        InvalidValue::new(format!(
            "expected an IP address such as 0.0.0.0 or ::, got {raw:?}"
        ))
    })
}

/// What a lookup yields once its problems are recorded.
enum Lookup {
    Unset,
    Set(String),
    Unusable,
}

struct Reader<F> {
    lookup: F,
    problems: Vec<VarProblem>,
}

impl<F> Reader<F>
where
    F: FnMut(&str) -> Result<String, VarError>,
{
    fn lookup(&mut self, var: &'static str) -> Lookup {
        match (self.lookup)(var) {
            Err(VarError::NotPresent) => Lookup::Unset,
            Err(VarError::NotUnicode(_)) => self.refuse(VarProblem::NotUnicode { var }),
            Ok(value) if value.is_empty() => self.refuse(VarProblem::Empty { var }),
            Ok(value) => Lookup::Set(value),
        }
    }

    fn refuse(&mut self, problem: VarProblem) -> Lookup {
        self.problems.push(problem);
        Lookup::Unusable
    }

    fn required<T>(
        &mut self,
        var: &'static str,
        parse: fn(&str) -> Result<T, InvalidValue>,
    ) -> Option<T> {
        match self.lookup(var) {
            Lookup::Set(raw) => self.parse(var, &raw, parse),
            Lookup::Unset => {
                self.problems.push(VarProblem::Missing { var });
                None
            }
            Lookup::Unusable => None,
        }
    }

    fn optional<T>(
        &mut self,
        var: &'static str,
        default: T,
        parse: fn(&str) -> Result<T, InvalidValue>,
    ) -> Option<T> {
        match self.lookup(var) {
            Lookup::Set(raw) => self.parse(var, &raw, parse),
            Lookup::Unset => Some(default),
            Lookup::Unusable => None,
        }
    }

    fn parse<T>(
        &mut self,
        var: &'static str,
        raw: &str,
        parse: fn(&str) -> Result<T, InvalidValue>,
    ) -> Option<T> {
        match parse(raw) {
            Ok(value) => Some(value),
            Err(reason) => {
                self.problems.push(VarProblem::Invalid { var, reason });
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::ffi::OsString;

    use super::*;

    // A made-up credential, only to prove it never reaches a rendering.
    const PASSWORD: &str = "Zq8example0nly";

    fn complete() -> HashMap<&'static str, String> {
        HashMap::from([
            (env::ENVIRONMENT, "prod".to_string()),
            (env::PORT, "8004".to_string()),
            (
                env::DATABASE_URL,
                format!("postgres://app:{PASSWORD}@pg-rw:5432/charter"), // trufflehog:ignore
            ),
            (env::NATS_URL, "nats://nats:4222".to_string()),
        ])
    }

    fn read(vars: &HashMap<&'static str, String>) -> Result<BootEnv, BootEnvError> {
        BootEnv::from_lookup(|name| vars.get(name).cloned().ok_or(VarError::NotPresent))
    }

    #[test]
    fn reads_a_complete_environment() {
        let boot = read(&complete()).unwrap();
        assert_eq!(boot.environment, Environment::Prod);
        assert_eq!(boot.port.get(), 8004);
        assert_eq!(boot.host, DEFAULT_HOST);
        assert_eq!(
            boot.database_url.as_str(),
            format!("postgres://app:{PASSWORD}@pg-rw:5432/charter") // trufflehog:ignore
        );
        assert_eq!(boot.nats_url.as_str(), "nats://nats:4222");
        assert_eq!(boot.listen_addr(), "0.0.0.0:8004".parse().unwrap());
    }

    #[test]
    fn host_binds_the_listener_when_set() {
        let mut vars = complete();
        vars.insert(env::HOST, "::1".to_string());
        assert_eq!(
            read(&vars).unwrap().listen_addr(),
            "[::1]:8004".parse().unwrap()
        );
    }

    #[test]
    fn debug_never_shows_the_database_password() {
        let debug = format!("{:?}", read(&complete()).unwrap());
        assert!(
            !debug.contains(PASSWORD),
            "Debug leaks the password: {debug}"
        );
        assert!(debug.contains("pg-rw"), "{debug}");
    }

    #[test]
    fn an_empty_environment_reports_every_required_variable() {
        let err = read(&HashMap::new()).unwrap_err();
        assert_eq!(
            err.problems(),
            [
                VarProblem::Missing {
                    var: env::ENVIRONMENT
                },
                VarProblem::Missing { var: env::PORT },
                VarProblem::Missing {
                    var: env::DATABASE_URL
                },
                VarProblem::Missing { var: env::NATS_URL },
            ]
        );
    }

    #[test]
    fn every_problem_is_reported_at_once() {
        let mut vars = complete();
        vars.insert(env::ENVIRONMENT, "staging".to_string());
        vars.insert(env::PORT, "0".to_string());
        vars.insert(env::HOST, "localhost".to_string());
        vars.insert(env::NATS_URL, String::new());
        let err = read(&vars).unwrap_err();
        let vars: Vec<_> = err.problems().iter().map(VarProblem::var).collect();
        assert_eq!(
            vars,
            [env::ENVIRONMENT, env::HOST, env::PORT, env::NATS_URL]
        );
        assert_eq!(err.problems()[3], VarProblem::Empty { var: env::NATS_URL });
        let message = err.to_string();
        assert!(message.contains("\"staging\""), "{message}");
        assert!(message.contains("got \"0\""), "{message}");
        assert!(message.contains("\"localhost\""), "{message}");
    }

    #[test]
    fn a_port_outside_the_tcp_range_is_refused() {
        for refused in ["0", "65536", "-1", "80a", " 8004"] {
            let mut vars = complete();
            vars.insert(env::PORT, refused.to_string());
            let err = read(&vars).unwrap_err();
            assert!(
                matches!(err.problems(), [VarProblem::Invalid { var, .. }] if *var == env::PORT),
                "{refused}: {err}"
            );
        }
    }

    #[test]
    fn a_non_unicode_value_is_refused_without_quoting_it() {
        let err = BootEnv::from_lookup(|name| {
            if name == env::DATABASE_URL {
                Err(VarError::NotUnicode(OsString::from(PASSWORD)))
            } else {
                complete().get(name).cloned().ok_or(VarError::NotPresent)
            }
        })
        .unwrap_err();
        assert_eq!(
            err.problems(),
            [VarProblem::NotUnicode {
                var: env::DATABASE_URL
            }]
        );
        assert!(!err.to_string().contains(PASSWORD));
    }

    #[test]
    fn an_invalid_database_url_is_refused_without_quoting_it() {
        let mut vars = complete();
        vars.insert(
            env::DATABASE_URL,
            format!("mysql://app:{PASSWORD}@db/charter"), // trufflehog:ignore
        );
        let err = read(&vars).unwrap_err();
        assert!(
            matches!(err.problems(), [VarProblem::Invalid { var, .. }] if *var == env::DATABASE_URL),
            "{err}"
        );
        assert!(!err.to_string().contains(PASSWORD), "{err}");
    }
}
