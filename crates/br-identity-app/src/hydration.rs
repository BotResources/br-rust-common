use br_core_scope::{ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};

use crate::error::AppError;

pub(crate) type ServiceRow = (String, String, String);
pub(crate) type ScopeRow = (String, String, String, String, bool);

pub(crate) fn build_hydration_input(
    service_rows: Vec<ServiceRow>,
    scope_rows: Vec<ScopeRow>,
) -> Result<Vec<(ServiceManifest, Vec<ScopeSpec>)>, AppError> {
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

fn parse_service_key(value: &str) -> Result<ServiceKey, AppError> {
    ServiceKey::new(value).map_err(|_| AppError::CorruptStoredKey {
        value: value.to_string(),
    })
}

fn parse_scope_key(value: &str) -> Result<ScopeKey, AppError> {
    ScopeKey::new(value).map_err(|_| AppError::CorruptStoredKey {
        value: value.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn build_hydration_input_rejects_corrupt_stored_key() {
        let services = vec![("BAD KEY".to_string(), "l".to_string(), "d".to_string())];
        let err = build_hydration_input(services, vec![]).unwrap_err();
        assert!(matches!(err, AppError::CorruptStoredKey { .. }));
    }
}
