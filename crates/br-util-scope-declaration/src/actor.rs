//! The declaring-service actor stamped on the declare command's metadata.
//!
//! ## Provenance, not authentication
//!
//! Every [`MessageMetadata`](br_core_integration::MessageMetadata) requires an
//! [`Actor`] (`Human(UserId) | Service(ServiceAccountId)`),
//! but at boot a service has **no authenticated principal**: the integration
//! bus is auth-less behind a default-deny NetworkPolicy, so there is nothing to
//! bind a declaration to. We therefore stamp a **deterministic, name-based**
//! service-account id derived from the declaring service's key:
//!
//! ```text
//! Actor::Service(ServiceAccountId(uuid_v5(DECLARING_SERVICE_NAMESPACE, service_key)))
//! ```
//!
//! This identifies *which service* (by convention) authored the declaration — it
//! is stable across reboots and replicas, collision-free per service key, and
//! greppable. It **authenticates nothing**: a peer with bus access could forge
//! the same id, exactly as it could forge any field. The honest guarantee is
//! "this is the conventional id of the named declarant", never "this proves the
//! sender is that service". Identity does not (and must not) trust this id as an
//! auth claim — there is intentionally no anti-spoof check in the contract
//! (see `br-core-scope`).

use br_core_integration::{Actor, ServiceAccountId};
use br_core_scope::ServiceKey;
use uuid::Uuid;

/// Fixed namespace for the v5 derivation of a declaring service's account id.
///
/// A constant of this crate so the derived id is reproducible from a service key
/// alone, anywhere, without coordination. Generated once (random v4) and frozen;
/// it has no meaning beyond namespacing the derivation, and changing it would
/// change every derived id, so it never changes.
const DECLARING_SERVICE_NAMESPACE: Uuid =
    Uuid::from_u128(0x6f3a_1c8e_4b27_4d59_9e10_a3f2_77c5_8d41);

/// The deterministic [`Actor`] stamped on a declaration published by `service`.
///
/// `Service(ServiceAccountId(uuid_v5(namespace, service_key)))` — see the
/// [module docs](crate::actor) for why this is *declarative provenance*, not
/// authentication.
pub fn declaring_actor(service: &ServiceKey) -> Actor {
    let id = Uuid::new_v5(&DECLARING_SERVICE_NAMESPACE, service.as_str().as_bytes());
    Actor::Service(ServiceAccountId::from(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The derived actor is deterministic: the same service key always yields the
    // same id (so reboots and replicas agree without coordination).
    #[test]
    fn actor_is_deterministic_per_service_key() {
        let key = ServiceKey::new("notifier").unwrap();
        let a = declaring_actor(&key);
        let b = declaring_actor(&key);
        assert_eq!(a, b);
        assert!(a.is_service());
    }

    // Distinct service keys derive distinct ids (collision-free per key).
    #[test]
    fn distinct_keys_yield_distinct_actors() {
        let notifier = declaring_actor(&ServiceKey::new("notifier").unwrap());
        let billing = declaring_actor(&ServiceKey::new("billing").unwrap());
        assert_ne!(notifier.id(), billing.id());
    }

    // It is a v5 (name-based) UUID under our namespace — pinned so the
    // derivation can't silently change shape (which would change the id and
    // break the stable-across-reboots property).
    #[test]
    fn derived_id_is_v5_under_the_crate_namespace() {
        let key = ServiceKey::new("notifier").unwrap();
        let expected = Uuid::new_v5(&DECLARING_SERVICE_NAMESPACE, b"notifier");
        assert_eq!(declaring_actor(&key).id(), expected);
        assert_eq!(expected.get_version_num(), 5);
    }
}
