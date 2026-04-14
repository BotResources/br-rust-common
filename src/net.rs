/// Check if a host is a trusted local address.
///
/// Matches `"localhost"`, `"127.0.0.1"`, `"::1"`, and any hostname listed
/// in the `TRUSTED_HOSTS` environment variable (comma-separated).
///
/// `TRUSTED_HOSTS` is designed for Docker Compose networking where service
/// hostnames (e.g. `postgres`, `nats`) are on an isolated internal network
/// and do not require TLS.
pub fn is_localhost(host: &str) -> bool {
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return true;
    }

    std::env::var("TRUSTED_HOSTS")
        .map(|val| val.split(',').any(|h| h.trim() == host))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_localhost() {
        assert!(is_localhost("localhost"));
    }

    #[test]
    fn matches_ipv4_loopback() {
        assert!(is_localhost("127.0.0.1"));
    }

    #[test]
    fn matches_ipv6_loopback() {
        assert!(is_localhost("::1"));
    }

    #[test]
    fn rejects_remote_host() {
        assert!(!is_localhost("db.example.com"));
        assert!(!is_localhost("10.0.0.1"));
    }
}
