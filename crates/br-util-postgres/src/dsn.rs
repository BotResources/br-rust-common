use url::{Host, Url};

use crate::error::PostgresError;

pub(crate) struct Dsn {
    url: Url,
    host: String,
}

impl Dsn {
    pub(crate) fn parse(raw: &str) -> Result<Self, PostgresError> {
        let url = Url::parse(raw).map_err(|e| {
            PostgresError::Config(format!(
                "could not parse the DSN to read its host for TLS validation: {e}"
            ))
        })?;

        if !matches!(url.scheme(), "postgres" | "postgresql") {
            return Err(PostgresError::Config(
                "the DSN scheme must be postgres:// or postgresql://".to_string(),
            ));
        }

        let host = match url.host() {
            Some(Host::Domain(domain)) if !domain.is_empty() => domain.to_string(),
            Some(Host::Ipv4(addr)) => addr.to_string(),
            Some(Host::Ipv6(addr)) => addr.to_string(),
            _ => {
                return Err(PostgresError::Config(
                    "the DSN authority has no host; TLS validation cannot vouch for the real \
                     target — put an explicit host in the URL authority"
                        .to_string(),
                ));
            }
        };

        Ok(Self { url, host })
    }

    pub(crate) fn host(&self) -> &str {
        &self.host
    }

    pub(crate) fn overrides_host(&self) -> bool {
        self.url
            .query_pairs()
            .any(|(k, _)| k == "host" || k == "hostaddr")
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use sqlx::postgres::PgConnectOptions;

    use super::*;

    fn with_userinfo(userinfo: &str, rest: &str) -> String {
        format!("postgres://{userinfo}@{rest}")
    }

    fn host_of(raw: &str) -> String {
        Dsn::parse(raw)
            .unwrap_or_else(|e| panic!("{raw} should parse: {e}"))
            .host()
            .to_string()
    }

    fn assert_config_error(raw: &str) {
        assert!(
            matches!(Dsn::parse(raw), Err(PostgresError::Config(_))),
            "{raw} must be refused with a config error"
        );
    }

    #[test]
    fn reads_host_from_standard_url() {
        assert_eq!(
            host_of(&with_userinfo("user:pass", "db.example.com:5432/mydb")),
            "db.example.com"
        );
    }

    #[test]
    fn reads_host_without_port() {
        assert_eq!(
            host_of("postgres://user@db.example.com/mydb"),
            "db.example.com"
        );
    }

    #[test]
    fn reads_host_without_credentials() {
        assert_eq!(host_of("postgres://localhost:5432/mydb"), "localhost");
    }

    #[test]
    fn reads_host_with_query() {
        assert_eq!(host_of("postgres://user@host/db?sslmode=require"), "host");
    }

    #[test]
    fn reads_host_with_no_path() {
        assert_eq!(host_of("postgres://user@db.example.com"), "db.example.com");
        assert_eq!(
            host_of("postgres://db.example.com?sslmode=require"),
            "db.example.com"
        );
    }

    #[test]
    fn reads_postgresql_scheme() {
        assert_eq!(host_of("postgresql://db.example.com/db"), "db.example.com");
    }

    #[test]
    fn reads_ipv6_host_without_brackets() {
        assert_eq!(host_of("postgres://user@[::1]:5432/mydb"), "::1");
        assert_eq!(host_of("postgres://[::1]/mydb"), "::1");
        assert_eq!(host_of("postgres://[2001:db8::1]:5432/mydb"), "2001:db8::1");
    }

    #[test]
    fn at_sign_in_query_does_not_change_the_host() {
        assert_eq!(
            host_of("postgres://evil.example.com/db?x=a@localhost"),
            "evil.example.com"
        );
        assert_eq!(
            host_of("postgres://evil.example.com?x=a@localhost"),
            "evil.example.com"
        );
    }

    #[test]
    fn at_sign_in_path_does_not_change_the_host() {
        assert_eq!(
            host_of("postgres://evil.example.com/a@localhost/db"),
            "evil.example.com"
        );
        assert_eq!(
            host_of("postgres://evil.example.com/@127.0.0.1"),
            "evil.example.com"
        );
    }

    #[test]
    fn at_sign_in_fragment_does_not_change_the_host() {
        assert_eq!(
            host_of("postgres://evil.example.com/db#a@localhost"),
            "evil.example.com"
        );
        assert_eq!(
            host_of("postgres://evil.example.com#@localhost"),
            "evil.example.com"
        );
    }

    #[test]
    fn percent_encoded_at_sign_in_userinfo_stays_in_the_credentials() {
        assert_eq!(
            host_of(&with_userinfo(
                "us%40localhost:pa%40ss",
                "evil.example.com/db"
            )),
            "evil.example.com"
        );
        assert_eq!(
            host_of(&with_userinfo(
                "user:p%40localhost",
                "db.example.com:5432/db"
            )),
            "db.example.com"
        );
    }

    #[test]
    fn last_at_sign_of_the_authority_separates_the_credentials() {
        assert_eq!(
            host_of(&with_userinfo("user:pa@localhost", "evil.example.com/db")),
            "evil.example.com"
        );
    }

    #[test]
    fn colon_in_password_does_not_change_the_host() {
        assert_eq!(
            host_of(&with_userinfo(
                "user:pa:ss:localhost",
                "evil.example.com:5432/db"
            )),
            "evil.example.com"
        );
    }

    #[test]
    fn comma_separated_hosts_are_not_split_into_a_loopback_prefix() {
        assert_eq!(
            host_of("postgres://localhost,evil.example.com/db"),
            "localhost,evil.example.com"
        );
    }

    #[test]
    fn missing_or_empty_host_is_refused() {
        assert_config_error("postgres://");
        assert_config_error("postgres:///db");
        assert_config_error("postgres://user@/db");
        assert_config_error(&with_userinfo("user:pass", ":5432/db"));
        assert_config_error("postgres://:5432/db");
        assert_config_error("postgres:db");
        assert_config_error("postgres:?host=localhost");
    }

    #[test]
    fn unparseable_dsn_is_refused() {
        assert_config_error("");
        assert_config_error("localhost");
        assert_config_error("not a url");
        assert_config_error("postgres://[::1/db");
        assert_config_error("postgres://localhost:notaport/db");
        assert_config_error("postgres://localhost:99999/db");
    }

    #[test]
    fn foreign_or_missing_scheme_is_refused() {
        assert_config_error("localhost/db");
        assert_config_error("localhost:5432/db");
        assert_config_error("http://localhost/db");
        assert_config_error("mysql://localhost/db");
        assert_config_error("evil.example.com/a@localhost");
    }

    #[test]
    fn host_override_parameters_are_detected() {
        assert!(
            Dsn::parse("postgres://localhost/db?host=evil.example.com")
                .unwrap()
                .overrides_host()
        );
        assert!(
            Dsn::parse("postgres://localhost/db?hostaddr=8.8.8.8")
                .unwrap()
                .overrides_host()
        );
        assert!(
            Dsn::parse("postgres://localhost/db?%68ost=evil.example.com")
                .unwrap()
                .overrides_host()
        );
        assert!(
            !Dsn::parse("postgres://localhost/db?application_name=svc")
                .unwrap()
                .overrides_host()
        );
    }

    #[test]
    fn extracted_host_is_the_host_the_driver_connects_to() {
        let corpus: Vec<String> = vec![
            with_userinfo("user:pass", "db.example.com:5432/mydb"),
            "postgres://evil.example.com/db?x=a@localhost".to_string(),
            "postgres://evil.example.com?x=a@localhost".to_string(),
            "postgres://evil.example.com/a@localhost/db".to_string(),
            "postgres://evil.example.com/db#a@localhost".to_string(),
            with_userinfo("us%40localhost:pa%40ss", "evil.example.com/db"),
            with_userinfo("user:pa@localhost", "evil.example.com/db"),
            with_userinfo("user:pa:ss:localhost", "evil.example.com:5432/db"),
            "postgres://evil.example.com\\@localhost/db".to_string(),
            "postgres://evil.example.com%2f@localhost/db".to_string(),
            "postgres://localhost,evil.example.com/db".to_string(),
            "postgres://[2001:db8::1]:5432/db".to_string(),
            "postgres://[::1]:5432/db".to_string(),
            "postgres://localhost:5432/db".to_string(),
        ];
        for raw in &corpus {
            let driver = PgConnectOptions::from_str(raw)
                .unwrap_or_else(|e| panic!("{raw}: driver refused: {e}"));
            let driver_host = driver
                .get_host()
                .trim_start_matches('[')
                .trim_end_matches(']');
            assert_eq!(host_of(raw), driver_host, "{raw}");
        }
    }
}
