//! The [`Actor`] that performs an action: a human user or a machine identity.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{ServiceAccountId, UserId};

/// Who performed an action — a human [`UserId`] or a machine [`ServiceAccountId`].
///
/// This is the typed replacement for the historical "actor is a bare `Uuid`"
/// shape: with only a `Uuid`, a human and a service account were
/// indistinguishable, so any downstream needing to branch on actor kind had to
/// guess. `Actor` carries the distinction in the type, while [`id`](Actor::id)
/// recovers the inner `Uuid` when the kind doesn't matter.
///
/// Its own serde shape is internally tagged (`{ "kind": "human", "id": "…" }`),
/// but envelopes that embed an `Actor` (e.g. `br_core_events::EventMetadata`)
/// flatten it onto their own wire format and own that contract — do not rely on
/// this tagged shape across a wire boundary; rely on the embedding type's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Actor {
    /// A human user.
    Human(UserId),
    /// A machine identity (service account).
    Service(ServiceAccountId),
}

impl Actor {
    /// The inner `Uuid`, whichever variant this is.
    ///
    /// Use when the actor's identity is needed but its kind is not — e.g.
    /// stamping `actor_id` onto a flat wire envelope. To branch on kind, match
    /// the variant or call [`is_human`](Actor::is_human) /
    /// [`is_service`](Actor::is_service).
    pub const fn id(&self) -> Uuid {
        match self {
            Self::Human(id) => id.as_uuid(),
            Self::Service(id) => id.as_uuid(),
        }
    }

    /// Whether this actor is a human user.
    pub const fn is_human(&self) -> bool {
        matches!(self, Self::Human(_))
    }

    /// Whether this actor is a machine identity (service account).
    pub const fn is_service(&self) -> bool {
        matches!(self, Self::Service(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_id_returns_inner_uuid() {
        let uuid = Uuid::from_u128(42);
        let actor = Actor::Human(UserId::from(uuid));
        assert_eq!(actor.id(), uuid);
    }

    #[test]
    fn service_id_returns_inner_uuid() {
        let uuid = Uuid::from_u128(7);
        let actor = Actor::Service(ServiceAccountId::from(uuid));
        assert_eq!(actor.id(), uuid);
    }

    #[test]
    fn is_human_discriminates() {
        let human = Actor::Human(UserId::from(Uuid::nil()));
        let service = Actor::Service(ServiceAccountId::from(Uuid::nil()));
        assert!(human.is_human());
        assert!(!human.is_service());
        assert!(service.is_service());
        assert!(!service.is_human());
    }

    #[test]
    fn human_serde_shape_is_tagged() {
        let actor = Actor::Human(UserId::from(Uuid::nil()));
        let json = serde_json::to_value(actor).unwrap();
        assert_eq!(json["kind"], "human");
        assert_eq!(json["id"], Uuid::nil().to_string());
    }

    #[test]
    fn service_serde_shape_is_tagged() {
        let actor = Actor::Service(ServiceAccountId::from(Uuid::nil()));
        let json = serde_json::to_value(actor).unwrap();
        assert_eq!(json["kind"], "service");
        assert_eq!(json["id"], Uuid::nil().to_string());
    }

    #[test]
    fn human_roundtrip() {
        let actor = Actor::Human(UserId::from(Uuid::from_u128(0x1234)));
        let json = serde_json::to_string(&actor).unwrap();
        let back: Actor = serde_json::from_str(&json).unwrap();
        assert_eq!(actor, back);
    }

    #[test]
    fn service_roundtrip() {
        let actor = Actor::Service(ServiceAccountId::from(Uuid::from_u128(0x1234)));
        let json = serde_json::to_string(&actor).unwrap();
        let back: Actor = serde_json::from_str(&json).unwrap();
        assert_eq!(actor, back);
    }

    #[test]
    fn human_and_service_with_same_uuid_are_distinct() {
        let uuid = Uuid::from_u128(99);
        let human = Actor::Human(UserId::from(uuid));
        let service = Actor::Service(ServiceAccountId::from(uuid));
        assert_ne!(human, service);
        assert_eq!(human.id(), service.id());
    }
}
