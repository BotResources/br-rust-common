//! Mapping persisted rows → the shape [`ScopeRegistry::hydrate`] consumes.
//!
//! This is the read-side of the adapter: it turns the raw `scope_registry_service`
//! and `scope_registry` rows into validated value objects grouped by owning
//! service, then the domain's [`hydrate`](br_identity_domain::ScopeRegistry::hydrate)
//! re-validates every cross-row invariant. Any store value that fails the
//! validated-type re-parse, or a scope filed under a service with no manifest
//! row, fails loud here — the read-side double barrier for store corruption.

use br_core_scope::{ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};

use crate::error::AppError;

/// A raw `scope_registry_service` row: `(service_key, label_key, description_key)`.
pub(crate) type ServiceRow = (String, String, String);
/// A raw `scope_registry` row:
/// `(scope_key, owning_service, label_key, description_key, platform_only)`.
pub(crate) type ScopeRow = (String, String, String, String, bool);

/// Group persisted scope rows under their owning service's manifest, in the
/// shape [`ScopeRegistry::hydrate`](br_identity_domain::ScopeRegistry::hydrate)
/// expects. A scope whose `owning_service` has no manifest row is a corrupt
/// store; hydration would not catch a *dangling* scope (it only groups under
/// known services), so it is rejected here as a missing-owner inconsistency
/// surfaced through the domain's hydration error.
pub(crate) fn build_hydration_input(
    service_rows: Vec<ServiceRow>,
    scope_rows: Vec<ScopeRow>,
) -> Result<Vec<(ServiceManifest, Vec<ScopeSpec>)>, AppError> {
    // Build manifests, keyed by service key, preserving load order.
    let mut services: Vec<(ServiceKey, ServiceManifest, Vec<ScopeSpec>)> =
        Vec::with_capacity(service_rows.len());
    for (key, label_key, description_key) in service_rows {
        let service_key = parse_service_key(&key)?;
        let manifest = ServiceManifest::new(service_key.clone(), label_key, description_key);
        services.push((service_key, manifest, Vec::new()));
    }

    for (scope_key, owning_service, label_key, description_key, platform_only) in scope_rows {
        let key = parse_scope_key(&scope_key)?;
        let owner = parse_service_key(&owning_service)?;
        let spec = ScopeSpec::new(key, label_key, description_key, platform_only);
        match services.iter_mut().find(|(k, _, _)| *k == owner) {
            Some((_, _, scopes)) => scopes.push(spec),
            // A scope filed under a service with no manifest row: the store is
            // missing the owner. Hydration groups under known services and would
            // silently drop this scope, so fail loud here instead — reuse the
            // domain's owner-mismatch signal to mark the inconsistency.
            None => {
                return Err(AppError::Hydration(
                    br_identity_domain::RegistryHydrationError::ScopeOwnerMismatch {
                        key: spec.key,
                        owning_service: owner,
                    },
                ));
            }
        }
    }

    Ok(services
        .into_iter()
        .map(|(_, manifest, scopes)| (manifest, scopes))
        .collect())
}

/// Parse a persisted service-key string back into the validated type. A stored
/// value that fails validation is a corrupt store; surface it as a fail-loud
/// [`AppError::CorruptStoredKey`] (the read-side barrier for key-syntax
/// corruption), never a panic.
fn parse_service_key(value: &str) -> Result<ServiceKey, AppError> {
    ServiceKey::new(value).map_err(|_| AppError::CorruptStoredKey {
        value: value.to_string(),
    })
}

/// Parse a persisted scope-key string back into the validated type.
fn parse_scope_key(value: &str) -> Result<ScopeKey, AppError> {
    ScopeKey::new(value).map_err(|_| AppError::CorruptStoredKey {
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // build_hydration_input groups scopes under their owning service in the
    // shape hydrate() consumes.
    #[test]
    fn build_hydration_input_groups_scopes_under_owner() {
        let services = vec![("notifier".to_string(), "l".to_string(), "d".to_string())];
        let scopes = vec![(
            "notifier:read".to_string(),
            "notifier".to_string(),
            "sl".to_string(),
            "sd".to_string(),
            false,
        )];
        let grouped = build_hydration_input(services, scopes).unwrap();
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].0.key.as_str(), "notifier");
        assert_eq!(grouped[0].1.len(), 1);
        assert_eq!(grouped[0].1[0].key.as_str(), "notifier:read");
    }

    // A scope filed under a service with no manifest row is a corrupt store:
    // fail loud as a hydration error rather than silently dropping it.
    #[test]
    fn build_hydration_input_rejects_orphan_scope() {
        let scopes = vec![(
            "ghost:read".to_string(),
            "ghost".to_string(),
            "l".to_string(),
            "d".to_string(),
            false,
        )];
        let err = build_hydration_input(vec![], scopes).unwrap_err();
        assert!(matches!(err, AppError::Hydration(_)));
    }

    // A malformed stored key fails the validated-type re-parse → CorruptStoredKey
    // (the read-side barrier for key-syntax corruption), never a panic.
    #[test]
    fn build_hydration_input_rejects_corrupt_stored_key() {
        let services = vec![("BAD KEY".to_string(), "l".to_string(), "d".to_string())];
        let err = build_hydration_input(services, vec![]).unwrap_err();
        assert!(matches!(err, AppError::CorruptStoredKey { .. }));
    }
}
