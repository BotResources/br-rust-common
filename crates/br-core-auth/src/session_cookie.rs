const SECURE_NAME: &str = "__Host-session_id";
const INSECURE_NAME: &str = "session_id";

/// Canonical cookie name for the session ID.
///
/// In production (`secure = true`) the cookie uses the `__Host-` prefix,
/// which browsers enforce as `Secure; Path=/; no Domain`. In local dev
/// (`secure = false`) the plain name is returned so the cookie works over
/// plain HTTP.
///
/// svc-auth sets this cookie; downstream services (svc-identity) read it
/// from the forwarded `Cookie` header via [`extract_session_id`].
pub fn session_cookie_name(secure: bool) -> &'static str {
    if secure { SECURE_NAME } else { INSECURE_NAME }
}

/// Extract the session ID value from a raw `Cookie` header string.
///
/// Parses a semicolon-separated cookie header and returns the value
/// associated with the canonical session cookie name (see
/// [`session_cookie_name`]).
pub fn extract_session_id(cookie_header: &str, secure: bool) -> Option<&str> {
    let name = session_cookie_name(secure);
    let prefix_len = name.len() + 1; // "name="
    cookie_header.split(';').find_map(|pair| {
        let pair = pair.trim();
        if pair.len() > prefix_len
            && pair.as_bytes()[name.len()] == b'='
            && pair[..name.len()].eq_ignore_ascii_case(name)
        {
            Some(&pair[prefix_len..])
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secure_name() {
        assert_eq!(session_cookie_name(true), "__Host-session_id");
    }

    #[test]
    fn insecure_name() {
        assert_eq!(session_cookie_name(false), "session_id");
    }

    #[test]
    fn extract_from_single_cookie() {
        let header = "session_id=abc-123";
        assert_eq!(extract_session_id(header, false), Some("abc-123"));
    }

    #[test]
    fn extract_from_multiple_cookies() {
        let header = "access_token=jwt; session_id=abc-123; refresh_token=rt";
        assert_eq!(extract_session_id(header, false), Some("abc-123"));
    }

    #[test]
    fn extract_secure_prefix() {
        let header = "__Host-session_id=uuid-value";
        assert_eq!(extract_session_id(header, true), Some("uuid-value"));
    }

    #[test]
    fn extract_missing_returns_none() {
        let header = "access_token=jwt; refresh_token=rt";
        assert_eq!(extract_session_id(header, false), None);
    }

    #[test]
    fn extract_empty_header_returns_none() {
        assert_eq!(extract_session_id("", false), None);
    }

    #[test]
    fn extract_wrong_secure_mode_returns_none() {
        let header = "session_id=abc";
        assert_eq!(extract_session_id(header, true), None);
    }

    #[test]
    fn extract_with_whitespace() {
        let header = "  session_id=abc  ";
        assert_eq!(extract_session_id(header, false), Some("abc"));
    }
}
