/// Whether a host is a loopback address (`"localhost"`, `"127.0.0.1"`, `"::1"`).
///
/// Loopback always sits on a trusted segment by definition, independent of any
/// configuration — so callers can short-circuit on it before touching the
/// environment.
pub(crate) fn is_loopback(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

/// Decide, **purely**, whether a host sits on a trusted network segment.
///
/// A host is trusted when it is loopback, or when `trusted` (the configured
/// trusted-network host list) contains it. No I/O, no environment read, no
/// side effects — the decision is a function of its two arguments only, which
/// is what makes it cheap to spec in env-free unit tests.
///
/// A match exempts the host from the remote-TLS requirement enforced by
/// [`crate::validate_database_tls`]. The list says what we actually trust: the
/// **network segment**, not the host. BR runs each service alongside its
/// CloudNativePG (CNPG) database in the same Kubernetes namespace, behind a
/// Kyverno-generated default-deny `NetworkPolicy`. App↔DB traffic is
/// intra-namespace, pod-to-pod, and intentionally plaintext — there is no
/// untrusted segment between them to protect with transport TLS. The trusted
/// list is how a service declares that its DB host lives on such a segment,
/// opting that specific host out of TLS. It is a deliberate, per-host,
/// conscious declaration — never a blanket bypass — so the lib stays
/// secure-by-default while letting the trusted-network deployment connect
/// plaintext.
pub(crate) fn is_on_trusted_network(host: &str, trusted: &[String]) -> bool {
    is_loopback(host) || trusted.iter().any(|h| h == host)
}

/// Resolve the configured trusted-network host list from the environment.
///
/// This is the **impure boundary**: it reads process-global env and emits the
/// deprecation warning. `TRUSTED_NETWORK_HOSTS` is the canonical name and wins
/// when set. When it is unset, `TRUSTED_HOSTS` is honored as a fallback and a
/// deprecation `tracing::warn!` is emitted on use. When neither is set, the
/// list is empty.
///
/// The value is parsed once here: split on `,`, trim each entry, drop empties.
///
/// `TRUSTED_HOSTS` is the deprecated former name; it is scheduled for removal
/// in `1.0.0`.
pub(crate) fn resolve_trusted_network_hosts() -> Vec<String> {
    let raw = match std::env::var("TRUSTED_NETWORK_HOSTS") {
        Ok(val) => Some(val),
        Err(_) => match std::env::var("TRUSTED_HOSTS") {
            Ok(val) => {
                tracing::warn!(
                    "TRUSTED_HOSTS is deprecated since 0.6.0 and will be removed \
                     in 1.0.0 — rename it to TRUSTED_NETWORK_HOSTS"
                );
                Some(val)
            }
            Err(_) => None,
        },
    };

    raw.map(|val| {
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

    // ─── pure decision: is_on_trusted_network(host, &trusted) ──
    //
    // The security decision is a pure function of (host, trusted), so it is
    // specced with env-free asserts — no process-global state, no guard.

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

    // ─── impure boundary: resolve_trusted_network_hosts() ──
    //
    // Env vars are process-global mutable state, so the env-touching tests
    // below set/restore the vars under an [`EnvGuard`], which holds [`ENV_LOCK`]
    // to serialize them against each other and restores prior values on drop
    // (even on panic-unwind) so no test leaks env state into a sibling.

    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard: captures `TRUSTED_NETWORK_HOSTS` / `TRUSTED_HOSTS` on
    /// construction, restores them on `Drop`, and holds [`ENV_LOCK`] for its
    /// lifetime so the env-touching tests serialize against each other rather
    /// than racing on the same process-global vars. A poisoned lock (a prior
    /// test panicked while holding it) is recovered, not propagated — the env
    /// is still restored on drop, so nothing to protect.
    struct EnvGuard {
        prior_new: Option<String>,
        prior_old: Option<String>,
        _lock: MutexGuard<'static, ()>,
    }

    impl EnvGuard {
        fn capture() -> Self {
            let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            Self {
                prior_new: std::env::var("TRUSTED_NETWORK_HOSTS").ok(),
                prior_old: std::env::var("TRUSTED_HOSTS").ok(),
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
                match &self.prior_old {
                    Some(v) => std::env::set_var("TRUSTED_HOSTS", v),
                    None => std::env::remove_var("TRUSTED_HOSTS"),
                }
            }
        }
    }

    #[test]
    fn resolve_honors_name_precedence_and_parsing() {
        let _guard = EnvGuard::capture();

        // Neither set → empty list.
        unsafe {
            std::env::remove_var("TRUSTED_NETWORK_HOSTS");
            std::env::remove_var("TRUSTED_HOSTS");
        }
        assert!(resolve_trusted_network_hosts().is_empty());

        // Canonical name → parsed into the configured list.
        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", "cnpg-rw, other-db");
        }
        assert_eq!(
            resolve_trusted_network_hosts(),
            vec!["cnpg-rw".to_string(), "other-db".to_string()]
        );

        // Deprecated alias alone → still honored as a fallback.
        unsafe {
            std::env::remove_var("TRUSTED_NETWORK_HOSTS");
            std::env::set_var("TRUSTED_HOSTS", "legacy-db");
        }
        assert_eq!(
            resolve_trusted_network_hosts(),
            vec!["legacy-db".to_string()]
        );

        // Canonical name wins when both are set (no fallback to the alias).
        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", "cnpg-rw");
            std::env::set_var("TRUSTED_HOSTS", "legacy-db");
        }
        assert_eq!(resolve_trusted_network_hosts(), vec!["cnpg-rw".to_string()]);

        // Prior env state is restored by `_guard` on drop, even on a panic
        // above.
    }

    /// The load-bearing fail-closed property, specced where it lives. Empty and
    /// whitespace-only entries are dropped, so the trusted list can never hold
    /// `""` — which is exactly what makes an unparseable host (it extracts to
    /// `""`) match no entry and therefore require TLS. Proven here at the
    /// resolver, not only end-to-end via `validate_database_tls`.
    #[test]
    fn resolve_drops_empty_and_whitespace_entries() {
        let _guard = EnvGuard::capture();

        // A list of only separators / whitespace resolves to nothing.
        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", ",  , ,");
        }
        assert!(resolve_trusted_network_hosts().is_empty());

        // Empties interleaved with real hosts are dropped; the rest survive.
        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", ", cnpg-rw ,, other-db , ");
        }
        assert_eq!(
            resolve_trusted_network_hosts(),
            vec!["cnpg-rw".to_string(), "other-db".to_string()]
        );
    }
}
