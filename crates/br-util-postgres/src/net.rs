pub(crate) fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

pub(crate) fn is_on_trusted_network(host: &str, trusted: &[String]) -> bool {
    is_loopback(host) || trusted.iter().any(|h| h == host)
}

pub(crate) fn resolve_trusted_network_hosts() -> Vec<String> {
    std::env::var("TRUSTED_NETWORK_HOSTS")
        .ok()
        .map(|val| {
            val.split(',')
                .map(str::trim)
                .filter(|h| !h.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_trusted_even_with_empty_list() {
        let trusted: Vec<String> = Vec::new();
        assert!(is_on_trusted_network("localhost", &trusted));
        assert!(is_on_trusted_network("127.0.0.1", &trusted));
        assert!(is_on_trusted_network("::1", &trusted));
    }

    #[test]
    fn listed_host_is_trusted() {
        let trusted = vec!["cnpg-rw".to_string(), "other-db".to_string()];
        assert!(is_on_trusted_network("cnpg-rw", &trusted));
        assert!(is_on_trusted_network("other-db", &trusted));
    }

    #[test]
    fn unlisted_remote_host_is_not_trusted() {
        let trusted = vec!["cnpg-rw".to_string()];
        assert!(!is_on_trusted_network("db.example.com", &trusted));
        assert!(!is_on_trusted_network("10.0.0.1", &trusted));
        assert!(!is_on_trusted_network("not-listed", &trusted));
    }

    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        prior_new: Option<String>,
        prior_legacy: Option<String>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            Self {
                prior_new: std::env::var("TRUSTED_NETWORK_HOSTS").ok(),
                prior_legacy: std::env::var("TRUSTED_HOSTS").ok(),
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior_new {
                    Some(v) => std::env::set_var("TRUSTED_NETWORK_HOSTS", v),
                    None => std::env::remove_var("TRUSTED_NETWORK_HOSTS"),
                }
                match &self.prior_legacy {
                    Some(v) => std::env::set_var("TRUSTED_HOSTS", v),
                    None => std::env::remove_var("TRUSTED_HOSTS"),
                }
            }
        }
    }

    #[test]
    fn resolve_reads_only_trusted_network_hosts_and_parses() {
        let _guard = EnvGuard::capture();

        unsafe {
            std::env::remove_var("TRUSTED_NETWORK_HOSTS");
        }
        assert!(resolve_trusted_network_hosts().is_empty());

        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", "cnpg-rw, other-db");
        }
        assert_eq!(
            resolve_trusted_network_hosts(),
            vec!["cnpg-rw".to_string(), "other-db".to_string()]
        );
    }

    #[test]
    fn resolve_ignores_legacy_trusted_hosts() {
        let _guard = EnvGuard::capture();

        unsafe {
            std::env::remove_var("TRUSTED_NETWORK_HOSTS");
            std::env::set_var("TRUSTED_HOSTS", "legacy-db");
        }
        assert!(resolve_trusted_network_hosts().is_empty());
    }

    #[test]
    fn resolve_drops_empty_and_whitespace_entries() {
        let _guard = EnvGuard::capture();

        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", ",  , ,");
        }
        assert!(resolve_trusted_network_hosts().is_empty());

        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", ", cnpg-rw ,, other-db , ");
        }
        assert_eq!(
            resolve_trusted_network_hosts(),
            vec!["cnpg-rw".to_string(), "other-db".to_string()]
        );
    }
}
