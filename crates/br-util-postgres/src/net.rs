/// Check if a host sits on a trusted network segment.
///
/// Matches the loopback addresses `"localhost"`, `"127.0.0.1"`, `"::1"`, and
/// any hostname listed in the `TRUSTED_NETWORK_HOSTS` environment variable
/// (comma-separated). A match exempts the host from the remote-TLS
/// requirement enforced by [`crate::validate_database_tls`].
///
/// The name says what we actually trust: the **network segment**, not the
/// host. BR runs each service alongside its CloudNativePG (CNPG) database in
/// the same Kubernetes namespace, behind a Kyverno-generated default-deny
/// `NetworkPolicy`. App↔DB traffic is intra-namespace, pod-to-pod, and
/// intentionally plaintext — there is no untrusted segment between them to
/// protect with transport TLS. `TRUSTED_NETWORK_HOSTS` is how a service
/// declares that its DB host lives on such a segment, opting that specific
/// host out of TLS. It is a deliberate, per-host, conscious declaration —
/// never a blanket bypass — so the lib stays secure-by-default while letting
/// the trusted-network deployment connect plaintext.
///
/// `TRUSTED_HOSTS` is the deprecated former name of this variable. It is
/// still honored as a fallback when `TRUSTED_NETWORK_HOSTS` is unset, and
/// emits a deprecation warning on use; it is scheduled for removal in
/// `1.0.0`.
pub(crate) fn is_on_trusted_network(host: &str) -> bool {
    if matches!(host, "localhost" | "127.0.0.1" | "::1") {
        return true;
    }

    let trusted = match std::env::var("TRUSTED_NETWORK_HOSTS") {
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

    trusted
        .map(|val| val.split(',').any(|h| h.trim() == host))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// Serializes the env-mutating test in *this module* against itself. The
    /// loopback cases below don't read the env, but a future env-touching test
    /// here must lock the same mutex. `pool.rs`'s env-reading tests are NOT
    /// coupled to this lock: they use `db.example.com`, which is in no trusted
    /// list, so they cannot collide with the values set here (non-collision by
    /// disjoint host values, not by shared serialization).
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// RAII guard that captures `TRUSTED_NETWORK_HOSTS` / `TRUSTED_HOSTS` on
    /// construction and restores them on `Drop` — so the restore runs even if
    /// an intermediate assertion panics and unwinds, never leaking mutated
    /// env state into a sibling test. It also holds [`ENV_LOCK`] for its whole
    /// lifetime, so both the serialization and the restore are released
    /// together. A poisoned lock (a prior test panicked while holding it) is
    /// recovered rather than propagated — the env is still restored on drop,
    /// so there is no real invariant to protect.
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
    fn matches_localhost() {
        assert!(is_on_trusted_network("localhost"));
    }

    #[test]
    fn matches_ipv4_loopback() {
        assert!(is_on_trusted_network("127.0.0.1"));
    }

    #[test]
    fn matches_ipv6_loopback() {
        assert!(is_on_trusted_network("::1"));
    }

    #[test]
    fn rejects_remote_host() {
        assert!(!is_on_trusted_network("db.example.com"));
        assert!(!is_on_trusted_network("10.0.0.1"));
    }

    /// Env vars are process-global mutable state, so the new-name /
    /// deprecated-alias / neither-set cases are exercised inside a single
    /// serialized test rather than three parallel ones that would race each
    /// other on the same variables. The [`EnvGuard`] both serializes against
    /// sibling env-mutators in this module and restores prior values on drop
    /// (including on panic-unwind), so this test never leaks state.
    #[test]
    fn trusted_network_hosts_and_legacy_alias() {
        let _guard = EnvGuard::capture();

        // Neither set → a non-loopback host is not trusted.
        unsafe {
            std::env::remove_var("TRUSTED_NETWORK_HOSTS");
            std::env::remove_var("TRUSTED_HOSTS");
        }
        assert!(!is_on_trusted_network("cnpg-rw"));

        // Canonical name → host is trusted.
        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", "cnpg-rw, other-db");
        }
        assert!(is_on_trusted_network("cnpg-rw"));
        assert!(is_on_trusted_network("other-db"));
        assert!(!is_on_trusted_network("not-listed"));

        // Deprecated alias alone → still honored as a fallback.
        unsafe {
            std::env::remove_var("TRUSTED_NETWORK_HOSTS");
            std::env::set_var("TRUSTED_HOSTS", "legacy-db");
        }
        assert!(is_on_trusted_network("legacy-db"));
        assert!(!is_on_trusted_network("cnpg-rw"));

        // Canonical name wins when both are set (no fallback to the alias).
        unsafe {
            std::env::set_var("TRUSTED_NETWORK_HOSTS", "cnpg-rw");
            std::env::set_var("TRUSTED_HOSTS", "legacy-db");
        }
        assert!(is_on_trusted_network("cnpg-rw"));
        assert!(!is_on_trusted_network("legacy-db"));

        // Prior env state is restored by `_guard` on drop, even on a panic
        // above.
    }
}
