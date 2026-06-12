use br_core_scope::{ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredService {
    manifest: ServiceManifest,
    scopes: Vec<ScopeSpec>,
}

impl RegisteredService {
    pub(crate) fn new(manifest: ServiceManifest) -> Self {
        Self {
            manifest,
            scopes: Vec::new(),
        }
    }

    pub(crate) fn push_scope(&mut self, spec: ScopeSpec) {
        self.scopes.push(spec);
    }

    pub(crate) fn owns_key(&self, key: &ScopeKey) -> bool {
        self.scopes.iter().any(|owned| &owned.key == key)
    }

    pub fn key(&self) -> &ServiceKey {
        &self.manifest.key
    }

    pub fn manifest(&self) -> &ServiceManifest {
        &self.manifest
    }

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

    #[test]
    fn new_service_owns_its_manifest_and_no_scopes() {
        let svc = service("notifier");
        assert_eq!(svc.key(), &ServiceKey::new("notifier").unwrap());
        assert!(svc.scopes().is_empty());
    }

    #[test]
    fn owns_key_is_keyed_on_the_scope_key_alone() {
        let mut svc = service("notifier");
        svc.push_scope(spec("notifier:read"));
        assert!(svc.owns_key(&ScopeKey::new("notifier:read").unwrap()));
        assert!(!svc.owns_key(&ScopeKey::new("notifier:write").unwrap()));
        assert!(svc.owns_key(&ScopeKey::new("notifier:read").unwrap()));
    }
}
