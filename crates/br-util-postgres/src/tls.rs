use std::str::FromStr;

use sqlx::postgres::{PgConnectOptions, PgSslMode};

use crate::dsn::Dsn;
use crate::env::TRUSTED_NETWORK_HOSTS;
use crate::error::PostgresError;
use crate::net::{is_loopback, is_on_trusted_network, resolve_trusted_network_hosts};

fn parse_sslmode(url: &str) -> Result<PgSslMode, PostgresError> {
    PgConnectOptions::from_str(url)
        .map(|opts| opts.get_ssl_mode())
        .map_err(|e| {
            PostgresError::Config(format!("could not parse the DSN for TLS validation: {e}"))
        })
}

pub fn validate_database_tls(url: &str) -> Result<(), PostgresError> {
    let dsn = Dsn::parse(url)?;

    if dsn.overrides_host() {
        return Err(PostgresError::Config(
            "the DSN overrides the target host via a host=/hostaddr= query \
             parameter; TLS validation cannot vouch for the real target — put \
             the host in the URL authority"
                .to_string(),
        ));
    }

    let host = dsn.host();
    let ssl_mode = parse_sslmode(url)?;

    if is_loopback(host) {
        return Ok(());
    }

    if is_on_trusted_network(host, &resolve_trusted_network_hosts()) {
        return Ok(());
    }

    let has_tls = matches!(
        ssl_mode,
        PgSslMode::Require | PgSslMode::VerifyCa | PgSslMode::VerifyFull
    );

    if !has_tls {
        return Err(PostgresError::Config(format!(
            "remote database connection to '{host}' requires TLS: \
             add sslmode=require (or verify-ca/verify-full) to the DSN, \
             or declare the host in {TRUSTED_NETWORK_HOSTS} if it sits on a \
             trusted network segment"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests;
