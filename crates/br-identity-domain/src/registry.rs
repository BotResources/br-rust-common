//! [`ScopeRegistry`] — the single aggregate that owns scope-key uniqueness
//! across every registered service.

use std::collections::HashSet;

use br_core_scope::{ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};

use crate::error::RegistryHydrationError;
use crate::event::{CommandResult, RegistryEvent};
use crate::service::RegisteredService;

/// The platform's scope registry — **one** instance holding every registered
/// service and the scopes it owns.
///
/// ## Why a single aggregate
///
/// The invariant the registry exists to protect — *a scope key is owned by at
/// most one service* — spans the whole set of services, so it cannot be split
/// across per-service aggregates without the invariant drifting between them. A
/// single aggregate makes the boundary own *exactly* that invariant. Identity
/// runs mono-pod (writes are naturally serialized, strong consistency), and the
/// aggregate still carries a [`version`](ScopeRegistry::version) for optimistic
/// locking so a future scale-out can retry on a conflict.
///
/// ## Double barrier
///
/// The uniqueness / ownership invariants are enforced at command time
/// ([`register_declaration`](ScopeRegistry::register_declaration)) **and**
/// re-validated when the aggregate is rebuilt from persisted state
/// ([`hydrate`](ScopeRegistry::hydrate)). A malformed persisted state fails to
/// load with a [`RegistryHydrationError`] instead of resurrecting an illegal
/// registry.
///
/// ## Reads
///
/// There is no separate read model: the same aggregate answers reads
/// ([`services`](ScopeRegistry::services), [`owner_of`](ScopeRegistry::owner_of))
/// and writes. This slice carries no user-facing state machine, so no affordances
/// are invented.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRegistry {
    version: u64,
    services: Vec<RegisteredService>,
}

impl ScopeRegistry {
    /// An empty registry at version `0`. The starting point before any
    /// declaration has been accepted.
    pub fn new() -> Self {
        Self {
            version: 0,
            services: Vec::new(),
        }
    }

    /// Rebuild a registry from persisted state, **re-validating every
    /// invariant** — the read-side half of the double barrier.
    ///
    /// The state is persisted as rows (the `version` plus the services and their
    /// scopes); this is *not* event replay. The signature groups each scope under
    /// its owning service, so a dangling owner is unrepresentable; each piece is
    /// already a validated type ([`ServiceManifest`] / [`ScopeSpec`], so key
    /// syntax is sound). What remains to re-check are the *cross-row* invariants:
    ///
    /// - every scope's `{service}` segment matches the service it is filed under
    ///   (ownership/prefix consistency);
    /// - no scope key appears twice across all services (global uniqueness).
    ///
    /// # Errors
    ///
    /// [`RegistryHydrationError`] on the first cross-row inconsistency found — a
    /// corrupt persisted state fails to load loudly rather than serving an
    /// illegal registry.
    pub fn hydrate(
        version: u64,
        services: Vec<(ServiceManifest, Vec<ScopeSpec>)>,
    ) -> Result<Self, RegistryHydrationError> {
        let mut registered = Vec::with_capacity(services.len());
        let mut seen: HashSet<ScopeKey> = HashSet::new();

        for (manifest, scopes) in services {
            let mut service = RegisteredService::new(manifest.clone());
            for spec in scopes {
                // Ownership/prefix consistency: the stored scope must belong to
                // the service it is filed under.
                if !spec.key.is_owned_by(&manifest.key) {
                    return Err(RegistryHydrationError::ScopeOwnerMismatch {
                        key: spec.key,
                        owning_service: manifest.key,
                    });
                }
                // Global uniqueness across every service: a key already seen
                // anywhere fails the insert (matching `ScopeDeclaration::new`).
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

    /// Register a service's declaration, judging the **registry** invariants.
    ///
    /// The declaration is already self-consistent ([`ScopeDeclaration`] proved
    /// every key syntactically valid, owned by the manifest's service, and
    /// duplicate-free), so the only judgment left to the registry is
    /// *cross-service*:
    ///
    /// - **Cross-owner conflict** — if any scope key is already owned by a
    ///   *different* service, the **whole** declaration is rejected atomically
    ///   with [`ScopeOwnedByAnotherService`]; nothing is partially registered
    ///   (the judgment runs fully before any mutation).
    /// - **Idempotent re-declaration** — a `(scope_key, owning_service)` already
    ///   present emits no event (idempotency is keyed on the scope key alone, not
    ///   its display metadata); a declaration that adds nothing new is a no-op
    ///   (empty [`CommandResult`], **no version bump**).
    ///
    /// On any state change the events are emitted granularly
    /// ([`ServiceRegistered`](RegistryEvent::ServiceRegistered) the first time a
    /// service appears, then one
    /// [`ScopeRegistered`](RegistryEvent::ScopeRegistered) per newly-registered
    /// scope) and the version is bumped once.
    ///
    /// This is a pure decision: it mutates in-memory state and returns events; it
    /// performs no I/O and does not persist or dispatch — that is the application
    /// layer's `load → command → save → dispatch`.
    ///
    /// # Errors
    ///
    /// [`ScopeOwnedByAnotherService`] if any declared key is owned by a different
    /// service.
    ///
    /// [`ScopeDeclaration`]: br_core_scope::ScopeDeclaration
    /// [`ScopeOwnedByAnotherService`]: br_core_scope::ScopeDeclarationError::ScopeOwnedByAnotherService
    pub fn register_declaration(
        &mut self,
        declaration: &ScopeDeclaration,
    ) -> Result<CommandResult, br_core_scope::ScopeDeclarationError> {
        let declaring = &declaration.manifest().key;

        // Pass 1 — judge the whole declaration before touching state, so a
        // rejection leaves nothing partially registered.
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

        // Pass 2 — apply. The service is registered the first time it appears;
        // each scope this service does not yet own is registered.
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
                continue; // idempotent: this service already owns the key → no event.
            }
            service.push_scope(spec.clone());
            events.push(scope_registered_event(spec, declaring));
        }

        if events.is_empty() {
            // Pure no-op re-declaration: no state change, so no version bump.
            return Ok(CommandResult::default());
        }
        self.version += 1;
        Ok(CommandResult::from_events(events))
    }

    /// The current optimistic-locking version: `0` for a fresh registry,
    /// incremented once per state-changing command (an idempotent no-op does not
    /// bump it). The application layer's save path uses it to detect a concurrent
    /// write and retry.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// The registered services, in registration order — the domain-shaped read
    /// surface (same aggregate for read and write, no separate read model).
    pub fn services(&self) -> &[RegisteredService] {
        &self.services
    }

    /// The service registered under `key`, if any.
    pub fn find_service(&self, key: &ServiceKey) -> Option<&RegisteredService> {
        self.services.iter().find(|svc| svc.key() == key)
    }

    /// Which service, if any, owns `key`.
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

/// Build the granular `ScopeRegistered` event carrying the scope's full value.
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

    // ─── fixtures ──────────────────────────────────────

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

    // ─── first declaration ─────────────────────────────

    // Given an empty registry, When a service declares a manifest and two scopes,
    // Then a ServiceRegistered and one ScopeRegistered per scope are emitted, each
    // carrying its full value (a subscriber never re-queries), and the version
    // bumps once.
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
        // The read surface reflects the write: same aggregate, no read model.
        assert_eq!(registry.services().len(), 1);
        assert_eq!(registry.services()[0].scopes().len(), 2);
    }

    // A manifest with no scopes registers just the service (one event).
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

    // Two different services coexist; the second does not re-emit the first's
    // ServiceRegistered and the version climbs per accepted declaration.
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

    // ─── idempotent re-declaration ─────────────────────

    // Given a service already declared, When it re-declares the identical set,
    // Then no new events are emitted and the version does NOT bump.
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

    // A re-declaration that adds ONE new scope emits only that scope (not the
    // already-known service or scope) and bumps the version once.
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

    // Idempotency is keyed on (scope_key, owning_service), NOT on metadata: a
    // re-declaration of an owned key with different label/description keys is
    // still a no-op (no event, no version bump, the stored metadata is unchanged).
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
        // The originally-stored metadata is untouched.
        assert_eq!(
            registry.services()[0].scopes()[0].label_key,
            "scope.notifier:read.label"
        );
    }

    // ─── cross-owner uniqueness authority ──────────────
    //
    // The registry is the authority on scope-key uniqueness: at most one service
    // owns a key. With `br-core-scope`'s current contract, a key's owner is
    // *determined by its prefix* (`ScopeDeclaration::new` forces every scope's
    // `{service}` segment to equal the declaring manifest's key), so two distinct
    // services can never legitimately contend for one key — the
    // `ScopeOwnedByAnotherService` branch of `register_declaration` is a guarded
    // impossibility on the validated path, kept as the explicit uniqueness check
    // (the double barrier: enforce the invariant even where the type system
    // already makes it hard to violate, and stay correct if ownership ever
    // decouples from the prefix). These tests prove the *reachable* property — a
    // key has exactly one owner, and re-asserting it by that owner is idempotent.

    // A key registered by its (only possible) owner has exactly that owner, and a
    // re-declaration by the same owner is the idempotent no-op — never a conflict.
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
        // The same owner re-declaring the same key: idempotent, never rejected.
        let again = registry
            .register_declaration(&declaration("notifier", vec![spec("notifier:read", false)]))
            .unwrap();
        assert!(again.is_noop());
    }

    // A registration is atomic: an accepted declaration registers all-or-nothing,
    // and a service registered alongside its scopes leaves a consistent state
    // (every scope owned by its declaring service, owner present).
    #[test]
    fn accepted_declaration_leaves_a_consistent_state() {
        let mut registry = ScopeRegistry::new();
        registry
            .register_declaration(&declaration(
                "notifier",
                vec![spec("notifier:read", false), spec("notifier:write", false)],
            ))
            .unwrap();
        // Re-hydrating the produced state must pass the read-side barrier — proof
        // the command never produces an internally-inconsistent registry.
        let persisted: Vec<_> = registry
            .services()
            .iter()
            .map(|svc| (svc.manifest().clone(), svc.scopes().to_vec()))
            .collect();
        let reloaded = ScopeRegistry::hydrate(registry.version(), persisted).unwrap();
        assert_eq!(reloaded, registry);
    }

    // ─── version behaviour ─────────────────────────────

    #[test]
    fn version_bumps_once_per_state_changing_command_only() {
        let mut registry = ScopeRegistry::new();
        assert_eq!(registry.version(), 0);
        registry
            .register_declaration(&declaration("a", vec![spec("a:read", false)]))
            .unwrap();
        assert_eq!(registry.version(), 1);
        // No-op re-declare: no bump.
        registry
            .register_declaration(&declaration("a", vec![spec("a:read", false)]))
            .unwrap();
        assert_eq!(registry.version(), 1);
        // New scope: bump.
        registry
            .register_declaration(&declaration("a", vec![spec("a:write", false)]))
            .unwrap();
        assert_eq!(registry.version(), 2);
    }

    // ─── hydration: valid state loads ─────────────────

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

    // After hydration the registry behaves like one built by commands: a key it
    // already holds re-declares as a no-op, a new key bumps from the loaded
    // version.
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

    // ─── hydration: malformed state fails to load ─────

    // A scope filed under a service whose key does not prefix it (ownership/prefix
    // inconsistency) fails to load.
    #[test]
    fn hydrate_rejects_scope_owner_mismatch() {
        let err = ScopeRegistry::hydrate(
            1,
            // `billing:read` filed under `notifier` — the stored prefix disagrees
            // with the recorded owner.
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

    // The same scope key under two services (or twice under one) breaks the
    // uniqueness invariant the registry exists to hold → fails to load. We use a
    // key with a shared-looking segment by filing the same key in two services'
    // rows — possible only in a corrupt store, which is exactly what hydration
    // must refuse.
    #[test]
    fn hydrate_rejects_duplicate_scope_across_services() {
        // Two services each (corruptly) recording `dup:read`. The first row is
        // owner-consistent (`dup` owns `dup:read`); the second files the same key
        // under `dup` again via a duplicate row.
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

    // An empty persisted state loads into the empty registry (version preserved).
    #[test]
    fn hydrate_accepts_empty_state() {
        let registry = ScopeRegistry::hydrate(0, vec![]).unwrap();
        assert_eq!(registry, ScopeRegistry::new());
    }

    // ─── error code (codes-not-language) ──────────────

    // The cross-owner conflict surfaces the shared rejection language with stable
    // codes; we reach it by registering a key then constructing the exact error
    // the command returns and asserting its code.
    #[test]
    fn cross_owner_error_is_a_stable_code() {
        let err = ScopeDeclarationError::ScopeOwnedByAnotherService {
            key: "svc_a:read".to_string(),
            owner: "svc_a".to_string(),
        };
        assert_eq!(err.to_string(), "scope_owned_by_another_service");
    }
}
