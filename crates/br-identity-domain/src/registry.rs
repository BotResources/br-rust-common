use std::collections::HashSet;

use br_core_scope::{ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};

use crate::error::RegistryHydrationError;
use crate::event::{CommandResult, RegistryEvent};
use crate::service::RegisteredService;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRegistry {
    version: u64,
    services: Vec<RegisteredService>,
}

impl ScopeRegistry {
    pub fn new() -> Self {
        Self {
            version: 0,
            services: Vec::new(),
        }
    }

    pub fn hydrate(
        version: u64,
        services: Vec<(ServiceManifest, Vec<ScopeSpec>)>,
    ) -> Result<Self, RegistryHydrationError> {
        let mut registered = Vec::with_capacity(services.len());
        let mut seen: HashSet<ScopeKey> = HashSet::new();

        for (manifest, scopes) in services {
            let mut service = RegisteredService::new(manifest.clone());
            for spec in scopes {
                if !spec.key.is_owned_by(&manifest.key) {
                    return Err(RegistryHydrationError::ScopeOwnerMismatch {
                        key: spec.key,
                        owning_service: manifest.key,
                    });
                }
                if !seen.insert(spec.key.clone()) {
                    return Err(RegistryHydrationError::DuplicateScope { key: spec.key });
                }
                service.push_scope(spec);
            }
            registered.push(service);
        }

        Ok(Self {
            version,
            services: registered,
        })
    }

    pub fn register_declaration(
        &mut self,
        declaration: &ScopeDeclaration,
    ) -> Result<CommandResult, br_core_scope::ScopeDeclarationError> {
        let declaring = &declaration.manifest().key;

        for spec in declaration.scopes() {
            if let Some(owner) = self.owner_of(&spec.key)
                && owner != declaring
            {
                return Err(
                    br_core_scope::ScopeDeclarationError::ScopeOwnedByAnotherService {
                        key: spec.key.as_str().to_string(),
                        owner: owner.as_str().to_string(),
                    },
                );
            }
        }

        let mut events = Vec::new();

        if self.find_service(declaring).is_none() {
            let manifest = declaration.manifest();
            self.services.push(RegisteredService::new(manifest.clone()));
            events.push(RegistryEvent::ServiceRegistered {
                service: manifest.key.clone(),
                label_key: manifest.label_key.clone(),
                description_key: manifest.description_key.clone(),
            });
        }

        for spec in declaration.scopes() {
            let service = self
                .service_mut(declaring)
                .expect("declaring service was just ensured present");
            if service.owns_key(&spec.key) {
                continue;
            }
            service.push_scope(spec.clone());
            events.push(scope_registered_event(spec, declaring));
        }

        if events.is_empty() {
            return Ok(CommandResult::default());
        }
        self.version += 1;
        Ok(CommandResult::from_events(events))
    }

    pub fn version(&self) -> u64 {
        self.version
    }

    pub fn services(&self) -> &[RegisteredService] {
        &self.services
    }

    pub fn find_service(&self, key: &ServiceKey) -> Option<&RegisteredService> {
        self.services.iter().find(|svc| svc.key() == key)
    }

    pub fn owner_of(&self, key: &ScopeKey) -> Option<&ServiceKey> {
        self.services
            .iter()
            .find(|svc| svc.owns_key(key))
            .map(|svc| svc.key())
    }

    fn service_mut(&mut self, key: &ServiceKey) -> Option<&mut RegisteredService> {
        self.services.iter_mut().find(|svc| svc.key() == key)
    }
}

impl Default for ScopeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn scope_registered_event(spec: &ScopeSpec, owning_service: &ServiceKey) -> RegistryEvent {
    RegistryEvent::ScopeRegistered {
        key: spec.key.clone(),
        owning_service: owning_service.clone(),
        label_key: spec.label_key.clone(),
        description_key: spec.description_key.clone(),
        platform_only: spec.platform_only,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_scope::ScopeDeclarationError;

    fn service_key(key: &str) -> ServiceKey {
        ServiceKey::new(key).unwrap()
    }

    fn manifest(service: &str) -> ServiceManifest {
        ServiceManifest::new(
            service_key(service),
            format!("service.{service}.label"),
            format!("service.{service}.desc"),
        )
    }

    fn spec(key: &str, platform_only: bool) -> ScopeSpec {
        ScopeSpec::new(
            ScopeKey::new(key).unwrap(),
            format!("scope.{key}.label"),
            format!("scope.{key}.desc"),
            platform_only,
        )
    }

    fn declaration(service: &str, scopes: Vec<ScopeSpec>) -> ScopeDeclaration {
        ScopeDeclaration::new(manifest(service), scopes).unwrap()
    }

    #[test]
    fn first_declaration_emits_service_then_scopes_with_full_value() {
        let mut registry = ScopeRegistry::new();
        assert_eq!(registry.version(), 0);

        let decl = declaration(
            "notifier",
            vec![spec("notifier:read", false), spec("notifier:admin", true)],
        );
        let result = registry.register_declaration(&decl).unwrap();

        assert_eq!(
            result.events,
            vec![
                RegistryEvent::ServiceRegistered {
                    service: service_key("notifier"),
                    label_key: "service.notifier.label".to_string(),
                    description_key: "service.notifier.desc".to_string(),
                },
                RegistryEvent::ScopeRegistered {
                    key: ScopeKey::new("notifier:read").unwrap(),
                    owning_service: service_key("notifier"),
                    label_key: "scope.notifier:read.label".to_string(),
                    description_key: "scope.notifier:read.desc".to_string(),
                    platform_only: false,
                },
                RegistryEvent::ScopeRegistered {
                    key: ScopeKey::new("notifier:admin").unwrap(),
                    owning_service: service_key("notifier"),
                    label_key: "scope.notifier:admin.label".to_string(),
                    description_key: "scope.notifier:admin.desc".to_string(),
                    platform_only: true,
                },
            ]
        );
        assert!(result.warnings.is_empty());
        assert_eq!(registry.version(), 1);
        assert_eq!(registry.services().len(), 1);
        assert_eq!(registry.services()[0].scopes().len(), 2);
    }

    #[test]
    fn manifest_only_declaration_registers_just_the_service() {
        let mut registry = ScopeRegistry::new();
        let result = registry
            .register_declaration(&declaration("notifier", vec![]))
            .unwrap();
        assert_eq!(
            result.events,
            vec![RegistryEvent::ServiceRegistered {
                service: service_key("notifier"),
                label_key: "service.notifier.label".to_string(),
                description_key: "service.notifier.desc".to_string(),
            }]
        );
        assert_eq!(registry.version(), 1);
    }

    #[test]
    fn distinct_services_coexist_and_version_climbs() {
        let mut registry = ScopeRegistry::new();
        registry
            .register_declaration(&declaration("notifier", vec![spec("notifier:read", false)]))
            .unwrap();
        let result = registry
            .register_declaration(&declaration("billing", vec![spec("billing:read", false)]))
            .unwrap();

        assert!(matches!(
            result.events[0],
            RegistryEvent::ServiceRegistered { .. }
        ));
        assert_eq!(result.events.len(), 2);
        assert_eq!(registry.version(), 2);
        assert_eq!(registry.services().len(), 2);
    }

    #[test]
    fn identical_redeclaration_is_a_noop() {
        let mut registry = ScopeRegistry::new();
        let decl = declaration(
            "notifier",
            vec![spec("notifier:read", false), spec("notifier:admin", true)],
        );
        registry.register_declaration(&decl).unwrap();
        assert_eq!(registry.version(), 1);

        let result = registry.register_declaration(&decl).unwrap();
        assert!(result.is_noop());
        assert!(result.events.is_empty());
        assert_eq!(registry.version(), 1, "a no-op must not bump the version");
    }

    #[test]
    fn partial_redeclaration_emits_only_the_new_scope() {
        let mut registry = ScopeRegistry::new();
        registry
            .register_declaration(&declaration("notifier", vec![spec("notifier:read", false)]))
            .unwrap();

        let result = registry
            .register_declaration(&declaration(
                "notifier",
                vec![spec("notifier:read", false), spec("notifier:write", false)],
            ))
            .unwrap();

        assert_eq!(
            result.events,
            vec![RegistryEvent::ScopeRegistered {
                key: ScopeKey::new("notifier:write").unwrap(),
                owning_service: service_key("notifier"),
                label_key: "scope.notifier:write.label".to_string(),
                description_key: "scope.notifier:write.desc".to_string(),
                platform_only: false,
            }]
        );
        assert_eq!(registry.version(), 2);
        assert_eq!(registry.services()[0].scopes().len(), 2);
    }

    #[test]
    fn redeclaration_with_changed_metadata_is_still_a_noop() {
        let mut registry = ScopeRegistry::new();
        registry
            .register_declaration(&declaration("notifier", vec![spec("notifier:read", false)]))
            .unwrap();

        let changed = ScopeSpec::new(
            ScopeKey::new("notifier:read").unwrap(),
            "different.label",
            "different.desc",
            true,
        );
        let result = registry
            .register_declaration(&declaration("notifier", vec![changed]))
            .unwrap();

        assert!(result.is_noop());
        assert_eq!(registry.version(), 1);
        assert_eq!(
            registry.services()[0].scopes()[0].label_key,
            "scope.notifier:read.label"
        );
    }

    #[test]
    fn a_key_has_exactly_one_owner_and_reasserting_it_is_idempotent() {
        let mut registry = ScopeRegistry::new();
        registry
            .register_declaration(&declaration("notifier", vec![spec("notifier:read", false)]))
            .unwrap();
        assert_eq!(
            registry.owner_of(&ScopeKey::new("notifier:read").unwrap()),
            Some(&service_key("notifier")),
        );
        let again = registry
            .register_declaration(&declaration("notifier", vec![spec("notifier:read", false)]))
            .unwrap();
        assert!(again.is_noop());
    }

    #[test]
    fn accepted_declaration_leaves_a_consistent_state() {
        let mut registry = ScopeRegistry::new();
        registry
            .register_declaration(&declaration(
                "notifier",
                vec![spec("notifier:read", false), spec("notifier:write", false)],
            ))
            .unwrap();
        let persisted: Vec<_> = registry
            .services()
            .iter()
            .map(|svc| (svc.manifest().clone(), svc.scopes().to_vec()))
            .collect();
        let reloaded = ScopeRegistry::hydrate(registry.version(), persisted).unwrap();
        assert_eq!(reloaded, registry);
    }

    #[test]
    fn version_bumps_once_per_state_changing_command_only() {
        let mut registry = ScopeRegistry::new();
        assert_eq!(registry.version(), 0);
        registry
            .register_declaration(&declaration("a", vec![spec("a:read", false)]))
            .unwrap();
        assert_eq!(registry.version(), 1);
        registry
            .register_declaration(&declaration("a", vec![spec("a:read", false)]))
            .unwrap();
        assert_eq!(registry.version(), 1);
        registry
            .register_declaration(&declaration("a", vec![spec("a:write", false)]))
            .unwrap();
        assert_eq!(registry.version(), 2);
    }

    #[test]
    fn hydrate_loads_valid_state_and_preserves_version() {
        let registry = ScopeRegistry::hydrate(
            7,
            vec![
                (
                    manifest("notifier"),
                    vec![spec("notifier:read", false), spec("notifier:admin", true)],
                ),
                (manifest("billing"), vec![spec("billing:read", false)]),
            ],
        )
        .unwrap();
        assert_eq!(registry.version(), 7);
        assert_eq!(registry.services().len(), 2);
        assert_eq!(
            registry.owner_of(&ScopeKey::new("notifier:admin").unwrap()),
            Some(&service_key("notifier"))
        );
    }

    #[test]
    fn hydrated_registry_resumes_at_loaded_version() {
        let mut registry = ScopeRegistry::hydrate(
            5,
            vec![(manifest("notifier"), vec![spec("notifier:read", false)])],
        )
        .unwrap();
        registry
            .register_declaration(&declaration(
                "notifier",
                vec![spec("notifier:write", false)],
            ))
            .unwrap();
        assert_eq!(registry.version(), 6);
    }

    #[test]
    fn hydrate_rejects_scope_owner_mismatch() {
        let err = ScopeRegistry::hydrate(
            1,
            vec![(manifest("notifier"), vec![spec("billing:read", false)])],
        )
        .unwrap_err();
        assert_eq!(
            err,
            RegistryHydrationError::ScopeOwnerMismatch {
                key: ScopeKey::new("billing:read").unwrap(),
                owning_service: service_key("notifier"),
            }
        );
    }

    #[test]
    fn hydrate_rejects_duplicate_scope_across_services() {
        let err = ScopeRegistry::hydrate(
            1,
            vec![(
                manifest("dup"),
                vec![spec("dup:read", false), spec("dup:read", true)],
            )],
        )
        .unwrap_err();
        assert_eq!(
            err,
            RegistryHydrationError::DuplicateScope {
                key: ScopeKey::new("dup:read").unwrap(),
            }
        );
    }

    #[test]
    fn hydrate_accepts_empty_state() {
        let registry = ScopeRegistry::hydrate(0, vec![]).unwrap();
        assert_eq!(registry, ScopeRegistry::new());
    }

    #[test]
    fn cross_owner_error_is_a_stable_code() {
        let err = ScopeDeclarationError::ScopeOwnedByAnotherService {
            key: "svc_a:read".to_string(),
            owner: "svc_a".to_string(),
        };
        assert_eq!(err.to_string(), "scope_owned_by_another_service");
    }
}
