const SECURE_NAME: &str = "__Host-session_id";
const INSECURE_NAME: &str = "session_id";

pub fn session_cookie_name(secure: bool) -> &'static str {
    if secure { SECURE_NAME } else { INSECURE_NAME }
}

pub fn extract_session_id(cookie_header: &str, secure: bool) -> Option<&str> {
    let name = session_cookie_name(secure);
    let prefix_len = name.len() + 1;
    let mut found: Option<&str> = None;
    for pair in cookie_header.split(';') {
        let pair = pair.trim();
        if pair.len() > prefix_len
            && pair.as_bytes()[name.len()] == b'='
            && pair[..name.len()] == *name
        {
            if found.is_some() {
                return None;
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

    #[test]
    fn extract_wrong_case_name_returns_none() {
        let header = "SESSION_ID=evil";
        assert_eq!(extract_session_id(header, false), None);
    }

    #[test]
    fn extract_wrong_case_secure_prefix_returns_none() {
        assert_eq!(extract_session_id("__HOST-session_id=evil", true), None);
        assert_eq!(extract_session_id("__host-session_id=evil", true), None);
    }

    #[test]
    fn extract_duplicate_name_evil_first_returns_none() {
        let header = "session_id=evil; session_id=real";
        assert_eq!(extract_session_id(header, false), None);
    }

    #[test]
    fn extract_duplicate_name_real_first_returns_none() {
        let header = "session_id=real; session_id=evil";
        assert_eq!(extract_session_id(header, false), None);
    }

    #[test]
    fn extract_duplicate_name_secure_returns_none() {
        let header = "__Host-session_id=real; __Host-session_id=evil";
        assert_eq!(extract_session_id(header, true), None);
    }

    #[test]
    fn extract_near_name_returns_none() {
        assert_eq!(extract_session_id("xsession_id=evil", false), None);
        assert_eq!(extract_session_id("session_idx=evil", false), None);
    }
}
