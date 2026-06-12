use br_core_kernel::{Actor, ServiceAccountId, UserId};
use serde::de::Deserializer;
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventMetadata {
    pub actor: Actor,
    pub correlation_id: Uuid,
    pub causation_id: Option<Uuid>,
}

impl EventMetadata {
    pub fn new(actor: Actor, correlation_id: Uuid) -> Self {
        Self {
            actor,
            correlation_id,
            causation_id: None,
        }
    }

    #[must_use]
    pub fn with_causation(mut self, causation_id: Uuid) -> Self {
        self.causation_id = Some(causation_id);
        self
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ActorKindWire {
    Human,
    Service,
}

#[derive(Deserialize)]
struct MetadataWire {
    actor_id: Uuid,
    #[serde(default)]
    actor_kind: Option<ActorKindWire>,
    correlation_id: Uuid,
    #[serde(default)]
    causation_id: Option<Uuid>,
}

impl Serialize for EventMetadata {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let field_count = 3 + usize::from(self.causation_id.is_some());
        let mut s = serializer.serialize_struct("EventMetadata", field_count)?;
        s.serialize_field("actor_id", &self.actor.id())?;
        let kind = if self.actor.is_human() {
            "human"
        } else {
            "service"
        };
        s.serialize_field("actor_kind", kind)?;
        s.serialize_field("correlation_id", &self.correlation_id)?;
        if let Some(causation_id) = self.causation_id {
            s.serialize_field("causation_id", &causation_id)?;
        }
        s.end()
    }
}

impl<'de> Deserialize<'de> for EventMetadata {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = MetadataWire::deserialize(deserializer)?;
        let actor = match wire.actor_kind {
            Some(ActorKindWire::Service) => Actor::Service(ServiceAccountId::from(wire.actor_id)),
            Some(ActorKindWire::Human) | None => Actor::Human(UserId::from(wire.actor_id)),
        };
        Ok(Self {
            actor,
            correlation_id: wire.correlation_id,
            causation_id: wire.causation_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_kernel::{ServiceAccountId, UserId};

    fn human(uuid: Uuid) -> Actor {
        Actor::Human(UserId::from(uuid))
    }

    fn service(uuid: Uuid) -> Actor {
        Actor::Service(ServiceAccountId::from(uuid))
    }

    #[test]
    fn serialize_emits_flat_actor_id_and_kind() {
        let meta = EventMetadata::new(human(Uuid::from_u128(0xAB)), Uuid::from_u128(0xCD))
            .with_causation(Uuid::from_u128(0xEF));
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["actor_id"], Uuid::from_u128(0xAB).to_string());
        assert_eq!(json["actor_kind"], "human");
        assert_eq!(json["correlation_id"], Uuid::from_u128(0xCD).to_string());
        assert_eq!(json["causation_id"], Uuid::from_u128(0xEF).to_string());
        assert!(json.get("actor").is_none());
    }

    #[test]
    fn serialize_service_kind() {
        let meta = EventMetadata::new(service(Uuid::nil()), Uuid::nil());
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["actor_kind"], "service");
    }

    #[test]
    fn serialize_without_causation_skips_field() {
        let meta = EventMetadata::new(human(Uuid::nil()), Uuid::nil());
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("causation_id"));
    }

    #[test]
    fn legacy_payload_without_actor_kind_defaults_to_human() {
        let legacy = r#"{
            "actor_id":"00000000-0000-0000-0000-0000000000ab",
            "correlation_id":"00000000-0000-0000-0000-0000000000cd"
        }"#;
        let meta: EventMetadata = serde_json::from_str(legacy).unwrap();
        assert_eq!(meta.actor, human(Uuid::from_u128(0xAB)));
        assert_eq!(meta.correlation_id, Uuid::from_u128(0xCD));
        assert_eq!(meta.causation_id, None);
    }

    #[test]
    fn legacy_payload_with_null_actor_kind_defaults_to_human() {
        let legacy = r#"{
            "actor_id":"00000000-0000-0000-0000-0000000000ab",
            "actor_kind":null,
            "correlation_id":"00000000-0000-0000-0000-0000000000cd"
        }"#;
        let meta: EventMetadata = serde_json::from_str(legacy).unwrap();
        assert_eq!(meta.actor, human(Uuid::from_u128(0xAB)));
    }

    #[test]
    fn legacy_payload_with_causation_defaults_to_human() {
        let legacy = r#"{
            "actor_id":"00000000-0000-0000-0000-0000000000ab",
            "correlation_id":"00000000-0000-0000-0000-0000000000cd",
            "causation_id":"00000000-0000-0000-0000-0000000000ef"
        }"#;
        let meta: EventMetadata = serde_json::from_str(legacy).unwrap();
        assert!(meta.actor.is_human());
        assert_eq!(meta.causation_id, Some(Uuid::from_u128(0xEF)));
    }

    #[test]
    fn service_actor_kind_roundtrips() {
        let meta = EventMetadata::new(service(Uuid::from_u128(0x55)), Uuid::nil());
        let json = serde_json::to_string(&meta).unwrap();
        let back: EventMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.actor, service(Uuid::from_u128(0x55)));
        assert!(back.actor.is_service());
    }

    #[test]
    fn unknown_actor_kind_is_error() {
        let bad = r#"{
            "actor_id":"00000000-0000-0000-0000-0000000000ab",
            "actor_kind":"robot",
            "correlation_id":"00000000-0000-0000-0000-0000000000cd"
        }"#;
        let result: Result<EventMetadata, _> = serde_json::from_str(bad);
        assert!(result.is_err(), "unknown actor_kind must fail, not default");
    }

    #[test]
    fn actor_kind_discriminant_rejects_unknown_value() {
        assert!(serde_json::from_str::<ActorKindWire>("\"robot\"").is_err());
        assert!(serde_json::from_str::<ActorKindWire>("\"human\"").is_ok());
        assert!(serde_json::from_str::<ActorKindWire>("\"service\"").is_ok());
    }

    #[test]
    fn full_roundtrip_human() {
        let meta = EventMetadata::new(human(Uuid::from_u128(1)), Uuid::from_u128(2))
            .with_causation(Uuid::from_u128(3));
        let json = serde_json::to_string(&meta).unwrap();
        let back: EventMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn full_roundtrip_service_no_causation() {
        let meta = EventMetadata::new(service(Uuid::from_u128(9)), Uuid::from_u128(8));
        let json = serde_json::to_string(&meta).unwrap();
        let back: EventMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(meta, back);
    }

    #[test]
    fn new_defaults_causation_to_none() {
        let meta = EventMetadata::new(human(Uuid::nil()), Uuid::nil());
        assert_eq!(meta.causation_id, None);
    }

    #[test]
    fn with_causation_sets_field() {
        let meta =
            EventMetadata::new(human(Uuid::nil()), Uuid::nil()).with_causation(Uuid::from_u128(7));
        assert_eq!(meta.causation_id, Some(Uuid::from_u128(7)));
    }

    #[test]
    fn clone_is_independent() {
        let meta = EventMetadata::new(service(Uuid::from_u128(4)), Uuid::from_u128(5))
            .with_causation(Uuid::from_u128(6));
        let cloned = meta.clone();
        assert_eq!(cloned, meta);
    }
}
