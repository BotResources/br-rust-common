use std::fmt;
use std::str::FromStr;

use url::Url;

use crate::error::InvalidValue;

const REDACTED: &str = "***";

/// The Postgres DSN of the runtime role, the value of `DATABASE_URL`: a
/// `postgres://` or `postgresql://` URL that names a host.
///
/// It carries the role's password, so it has no `Display`, and its `Debug`
/// shows it redacted: formatting a [`BootEnv`](crate::BootEnv) into a log
/// cannot leak it. Only the shape is checked here; whether the host may be
/// reached without TLS is decided by `br_util_postgres::init_pool`, which
/// takes [`as_str`](Self::as_str).
#[derive(Clone, PartialEq, Eq)]
pub struct DatabaseUrl {
    raw: String,
    parsed: Url,
}

impl DatabaseUrl {
    /// The DSN exactly as it was set, password included — for the pool, never
    /// for a log.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The DSN with its password — in the authority, or as a `password` query
    /// parameter — replaced by `***`.
    pub fn redacted(&self) -> String {
        let mut url = self.parsed.clone();
        if url.password().is_some() && url.set_password(Some(REDACTED)).is_err() {
            return format!("{}://{REDACTED}", url.scheme());
        }
        if url.query_pairs().any(|(key, _)| key == "password") {
            let pairs: Vec<(String, String)> = url
                .query_pairs()
                .map(|(key, value)| {
                    let value = if key == "password" {
                        REDACTED.to_string()
                    } else {
                        value.into_owned()
                    };
                    (key.into_owned(), value)
                })
                .collect();
            url.query_pairs_mut().clear().extend_pairs(pairs);
        }
        url.into()
    }
}

impl FromStr for DatabaseUrl {
    type Err = InvalidValue;

    /// The refusal never quotes the value: it may carry a password.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed =
            Url::parse(s).map_err(|e| InvalidValue::new(format!("not a Postgres URL ({e})")))?;
        if !matches!(parsed.scheme(), "postgres" | "postgresql") {
            return Err(InvalidValue::new(format!(
                "the scheme must be postgres or postgresql, got {:?}",
                parsed.scheme()
            )));
        }
        if parsed.host_str().is_none_or(str::is_empty) {
            return Err(InvalidValue::new("the URL names no host"));
        }
        Ok(Self {
            raw: s.to_string(),
            parsed,
        })
    }
}

impl fmt::Debug for DatabaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("DatabaseUrl")
            .field(&self.redacted())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A made-up credential, only to prove it never reaches a rendering.
    const PASSWORD: &str = "Zq8example0nly";

    fn url(s: &str) -> DatabaseUrl {
        s.parse().unwrap()
    }

    #[test]
    fn keeps_the_dsn_exactly_as_set() {
        let raw = format!("postgres://app:{PASSWORD}@pg-rw:5432/charter"); // trufflehog:ignore
        assert_eq!(url(&raw).as_str(), raw);
    }

    #[test]
    fn accepts_both_schemes_and_an_ipv6_host() {
        url("postgresql://app@pg-rw/charter?sslmode=require");
        url("postgres://app@[::1]:5432/charter");
    }

    #[test]
    fn redacts_the_password_of_the_authority() {
        let dsn = url(&format!(
            "postgres://app:{PASSWORD}@pg-rw:5432/charter?sslmode=require" // trufflehog:ignore
        ));
        assert_eq!(
            dsn.redacted(),
            "postgres://app:***@pg-rw:5432/charter?sslmode=require" // trufflehog:ignore
        );
        let debug = format!("{dsn:?}");
        assert!(
            !debug.contains(PASSWORD),
            "Debug leaks the password: {debug}"
        );
    }

    #[test]
    fn redacts_a_password_query_parameter() {
        let dsn = url(&format!(
            "postgres://pg-rw/charter?user=app&password={PASSWORD}" // trufflehog:ignore
        ));
        let redacted = dsn.redacted();
        assert!(!redacted.contains(PASSWORD), "{redacted}");
        assert!(redacted.contains("user=app"), "{redacted}");
    }

    #[test]
    fn a_dsn_without_password_is_unchanged() {
        assert_eq!(
            url("postgres://app@pg-rw:5432/charter").redacted(),
            "postgres://app@pg-rw:5432/charter"
        );
    }

    #[test]
    fn refuses_another_scheme_without_quoting_the_value() {
        let err = format!("mysql://app:{PASSWORD}@db/charter") // trufflehog:ignore
            .parse::<DatabaseUrl>()
            .unwrap_err()
            .to_string();
        assert!(err.contains("\"mysql\""), "{err}");
        assert!(
            !err.contains(PASSWORD),
            "the refusal leaks the password: {err}"
        );
    }

    #[test]
    fn refuses_a_value_that_is_not_a_url_or_names_no_host() {
        for refused in ["pg-rw:5432/charter", "not a url", "postgres:///charter"] {
            assert!(refused.parse::<DatabaseUrl>().is_err(), "{refused}");
        }
    }
}
