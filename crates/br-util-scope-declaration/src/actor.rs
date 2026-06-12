use br_core_integration::{Actor, ServiceAccountId};
use br_core_scope::ServiceKey;
use uuid::Uuid;

const DECLARING_SERVICE_NAMESPACE: Uuid =
    Uuid::from_u128(0x6f3a_1c8e_4b27_4d59_9e10_a3f2_77c5_8d41);

pub fn declaring_actor(service: &ServiceKey) -> Actor {
    let id = Uuid::new_v5(&DECLARING_SERVICE_NAMESPACE, service.as_str().as_bytes());
    Actor::Service(ServiceAccountId::from(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_is_deterministic_per_service_key() {
        let key = ServiceKey::new("notifier").unwrap();
        let a = declaring_actor(&key);
        let b = declaring_actor(&key);
        assert_eq!(a, b);
        assert!(a.is_service());
    }

    #[test]
    fn distinct_keys_yield_distinct_actors() {
        let notifier = declaring_actor(&ServiceKey::new("notifier").unwrap());
        let billing = declaring_actor(&ServiceKey::new("billing").unwrap());
        assert_ne!(notifier.id(), billing.id());
    }

    #[test]
    fn derived_id_is_v5_under_the_crate_namespace() {
        let key = ServiceKey::new("notifier").unwrap();
        let expected = Uuid::new_v5(&DECLARING_SERVICE_NAMESPACE, b"notifier");
        assert_eq!(declaring_actor(&key).id(), expected);
        assert_eq!(expected.get_version_num(), 5);
    }
}
