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
            Some(Host::Domain("")) | None => {
                return Err(PostgresError::Config(
                    "the DSN authority has no host; TLS validation cannot vouch for the real \
                     target — put an explicit host in the URL authority"
                        .to_string(),
                ));
            }
            Some(_) => url
                .host_str()
                .unwrap_or_default()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string(),
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
mod tests;
