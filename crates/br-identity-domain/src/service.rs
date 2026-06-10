//! [`RegisteredService`] — a service entity inside the registry, with the
//! scopes it owns.

use br_core_scope::{ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};

/// A service known to the [`ScopeRegistry`](crate::ScopeRegistry): its manifest
/// plus the scopes it owns.
///
/// A first-class entity (not a flat list of keys) so the grant-admin read
/// surface can group scopes by their owning service. It is a **child of the
/// registry aggregate**: it is only ever mutated through the root, never
/// directly, and its scope collection is private — the registry's key-uniqueness
/// invariant spans *all* services, so no single service may be edited in
/// isolation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredService {
    manifest: ServiceManifest,
    scopes: Vec<ScopeSpec>,
}

impl RegisteredService {
    /// Create a registered service from its manifest, owning no scopes yet.
    /// Crate-internal: only the registry root may construct one.
    pub(crate) fn new(manifest: ServiceManifest) -> Self {
        Self {
            manifest,
            scopes: Vec::new(),
        }
    }

    /// Add a scope to this service. Crate-internal and unchecked here: the
    /// registry root has already established uniqueness and ownership before
    /// calling, so the entity stays a pure container.
    pub(crate) fn push_scope(&mut self, spec: ScopeSpec) {
        self.scopes.push(spec);
    }

    /// Whether this service already owns a scope under `key` — the idempotency
    /// test, keyed on `(scope_key, owning_service)` per the registry's
    /// uniqueness invariant. Display metadata is not part of the key: a
    /// re-declaration of a key this service already owns is a no-op regardless of
    /// whether its label/description keys differ.
    pub(crate) fn owns_key(&self, key: &ScopeKey) -> bool {
        self.scopes.iter().any(|owned| &owned.key == key)
    }

    /// The service's key.
    pub fn key(&self) -> &ServiceKey {
        &self.manifest.key
    }

    /// The service's manifest (key + i18n display metadata).
    pub fn manifest(&self) -> &ServiceManifest {
        &self.manifest
    }

    /// The scopes this service owns, in registration order.
    pub fn scopes(&self) -> &[ScopeSpec] {
        &self.scopes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(key: &str) -> ScopeSpec {
        ScopeSpec::new(ScopeKey::new(key).unwrap(), "l", "d", false)
    }

    fn service(key: &str) -> RegisteredService {
        RegisteredService::new(ServiceManifest::new(
            ServiceKey::new(key).unwrap(),
            "l",
            "d",
        ))
    }

    // A fresh service owns its manifest and no scopes.
    #[test]
    fn new_service_owns_its_manifest_and_no_scopes() {
        let svc = service("notifier");
        assert_eq!(svc.key(), &ServiceKey::new("notifier").unwrap());
        assert!(svc.scopes().is_empty());
    }

    // owns_key keys idempotency on the scope key alone, not on display metadata.
    #[test]
    fn owns_key_is_keyed_on_the_scope_key_alone() {
        let mut svc = service("notifier");
        svc.push_scope(spec("notifier:read"));
        assert!(svc.owns_key(&ScopeKey::new("notifier:read").unwrap()));
        assert!(!svc.owns_key(&ScopeKey::new("notifier:write").unwrap()));
        // A different ScopeSpec metadata for the same key is still "owned".
        assert!(svc.owns_key(&ScopeKey::new("notifier:read").unwrap()));
    }
}
