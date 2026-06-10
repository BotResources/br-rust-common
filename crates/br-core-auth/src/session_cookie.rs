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
///
/// The cookie name is matched **exactly and case-sensitively**, as required
/// by RFC 6265 (cookie names are case-sensitive). The legitimate producer
/// (svc-auth) always sets the exact name, so any other-case spelling is
/// necessarily third-party-injected and is rejected. This matters for the
/// `__Host-` prefix: the browser's `Secure; Path=/; no Domain` guarantees
/// apply only to the exact-case prefix, so honoring `__HOST-session_id`
/// would accept a cookie the browser never constrained.
///
/// **Duplicates are rejected (fail closed).** If the exact name matches more
/// than one pair in the header, the value is ambiguous — a duplicate is the
/// signature of a cookie-tossing / prefix-injection attempt, and there is no
/// safe way to pick a winner. Since this cookie bears identity, ambiguity
/// returns `None` rather than guessing.
pub fn extract_session_id(cookie_header: &str, secure: bool) -> Option<&str> {
    let name = session_cookie_name(secure);
    let prefix_len = name.len() + 1; // "name="
    let mut found: Option<&str> = None;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        // Order matters and is UTF-8-panic-safe: the `== b'='` byte check at
        // index `name.len()` proves that `name.len()` is a char boundary
        // before the `pair[..name.len()]` slice runs. Do not reorder or
        // "simplify" these conditions.
        if pair.len() > prefix_len
            && pair.as_bytes()[name.len()] == b'='
            && pair[..name.len()] == *name
        {
            if found.is_some() {
                return None; // duplicate name → ambiguous → fail closed
            }
            found = Some(&pair[prefix_len..]);
        }
    }
    found
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

    // Given a cookie whose name differs only in case from the insecure name
    // When extracting in insecure mode
    // Then it is rejected — names are case-sensitive (RFC 6265)
    #[test]
    fn extract_wrong_case_name_returns_none() {
        let header = "SESSION_ID=evil";
        assert_eq!(extract_session_id(header, false), None);
    }

    // Given a forged variant-case `__Host-` cookie
    // When extracting in secure mode
    // Then it is rejected — only the exact-case `__Host-` prefix carries the
    // browser's Secure/Path/no-Domain guarantees
    #[test]
    fn extract_wrong_case_secure_prefix_returns_none() {
        assert_eq!(extract_session_id("__HOST-session_id=evil", true), None);
        assert_eq!(extract_session_id("__host-session_id=evil", true), None);
    }

    // Given two cookies with the exact session name (evil injected first)
    // When extracting
    // Then ambiguity is rejected — fail closed
    #[test]
    fn extract_duplicate_name_evil_first_returns_none() {
        let header = "session_id=evil; session_id=real";
        assert_eq!(extract_session_id(header, false), None);
    }

    // Given two cookies with the exact session name (real first, evil appended)
    // When extracting
    // Then ambiguity is rejected — fail closed regardless of order
    #[test]
    fn extract_duplicate_name_real_first_returns_none() {
        let header = "session_id=real; session_id=evil";
        assert_eq!(extract_session_id(header, false), None);
    }

    // Given duplicate exact-name cookies in secure mode
    // When extracting
    // Then ambiguity is rejected — fail closed
    #[test]
    fn extract_duplicate_name_secure_returns_none() {
        let header = "__Host-session_id=real; __Host-session_id=evil";
        assert_eq!(extract_session_id(header, true), None);
    }

    // Given cookie names that merely contain the session name as a substring
    // When extracting
    // Then no match — the name must be the whole cookie name, not a prefix/suffix
    #[test]
    fn extract_near_name_returns_none() {
        assert_eq!(extract_session_id("xsession_id=evil", false), None);
        assert_eq!(extract_session_id("session_idx=evil", false), None);
    }
}
