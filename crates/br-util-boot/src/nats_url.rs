use std::fmt;
use std::str::FromStr;

use url::Url;

use crate::error::InvalidValue;

const REDACTED: &str = "***";

/// The URL of the NATS server, the value of `NATS_URL`: a `nats://`, `tls://`,
/// `ws://` or `wss://` URL that names a host — the schemes the NATS client
/// accepts, stated explicitly (the client would read a bare `host:port` as
/// `nats://`; this reader refuses it, so the transport is never implied).
///
/// Its user information may be a credential — a user and a password, or a
/// token in the user position — so it has no `Display`, and its `Debug` shows
/// the whole user information redacted. Pass [`as_str`](Self::as_str) to
/// `br_util_nats_fabric::Fabric::connect`.
#[derive(Clone, PartialEq, Eq)]
pub struct NatsUrl {
    raw: String,
    parsed: Url,
}

impl NatsUrl {
    /// The URL exactly as it was set, credentials included — for the client,
    /// never for a log.
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The URL with its whole user information replaced by `***`.
    pub fn redacted(&self) -> String {
        let mut url = self.parsed.clone();
        if (!url.username().is_empty() || url.password().is_some())
            && (url.set_password(None).is_err() || url.set_username(REDACTED).is_err())
        {
            return format!("{}://{REDACTED}", url.scheme());
        }
        url.into()
    }
}

impl FromStr for NatsUrl {
    type Err = InvalidValue;

    /// The refusal never quotes the value: it may carry a credential.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parsed =
            Url::parse(s).map_err(|e| InvalidValue::new(format!("not a NATS URL ({e})")))?;
        if !matches!(parsed.scheme(), "nats" | "tls" | "ws" | "wss") {
            return Err(InvalidValue::new(format!(
                "the scheme must be nats, tls, ws or wss, got {:?}",
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

impl fmt::Debug for NatsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("NatsUrl").field(&self.redacted()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made-up credentials, only to prove they never reach a rendering.
    const PASSWORD: &str = "Zq8example0nly";
    const TOKEN: &str = "t0kenExample0nly";

    fn url(s: &str) -> NatsUrl {
        s.parse().unwrap()
    }

    #[test]
    fn keeps_the_url_exactly_as_set() {
        assert_eq!(url("nats://nats:4222").as_str(), "nats://nats:4222");
    }

    #[test]
    fn accepts_every_client_scheme() {
        for scheme in ["nats", "tls", "ws", "wss"] {
            url(&format!("{scheme}://nats.example:4222"));
        }
    }

    #[test]
    fn redacts_a_user_and_password() {
        let nats = url(&format!("nats://svc:{PASSWORD}@nats:4222")); // trufflehog:ignore
        assert_eq!(nats.redacted(), "nats://***@nats:4222");
        let debug = format!("{nats:?}");
        assert!(
            !debug.contains(PASSWORD),
            "Debug leaks the password: {debug}"
        );
    }

    #[test]
    fn redacts_a_token_in_the_user_position() {
        let nats = url(&format!("tls://{TOKEN}@nats:4222")); // trufflehog:ignore
        assert_eq!(nats.redacted(), "tls://***@nats:4222");
    }

    #[test]
    fn a_url_without_credentials_is_unchanged() {
        assert_eq!(url("nats://nats:4222").redacted(), "nats://nats:4222");
    }

    #[test]
    fn refuses_a_bare_host_another_scheme_or_no_host() {
        for refused in [
            "nats:4222",
            "localhost:4222",
            "http://nats:4222",
            "nats://",
            "nats",
        ] {
            assert!(refused.parse::<NatsUrl>().is_err(), "{refused}");
        }
    }

    #[test]
    fn the_refusal_never_quotes_the_value() {
        let err = format!("http://svc:{PASSWORD}@nats:4222") // trufflehog:ignore
            .parse::<NatsUrl>()
            .unwrap_err()
            .to_string();
        assert!(
            !err.contains(PASSWORD),
            "the refusal leaks the password: {err}"
        );
    }
}
