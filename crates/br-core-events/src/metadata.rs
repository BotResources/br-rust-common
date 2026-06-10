//! [`EventMetadata`] — the identity / correlation context attached to every
//! event, plus its backward-compatible wire encoding.
//!
//! ## Wire contract (load-bearing — do not change lightly)
//!
//! The metadata is persisted as JSONB and travels on NATS. The Rust type
//! changed in 0.4.0 (`actor_id: Uuid` → `actor: Actor`), but the **wire format
//! stays backward-compatible**: it is a flat object
//!
//! ```json
//! { "actor_id": "<uuid>", "actor_kind": "human" | "service",
//!   "correlation_id": "<uuid>", "causation_id": "<uuid>"? }
//! ```
//!
//! - **Serialization** always emits `actor_id` + `actor_kind` (a new field),
//!   alongside `correlation_id` and the optional `causation_id` exactly as
//!   before.
//! - **Deserialization** reads `actor_id` and an **optional** `actor_kind`.
//!   When `actor_kind` is **absent** — every payload written before 0.4.0 —
//!   the actor defaults to [`Actor::Human`]: machine actors did not exist in
//!   this envelope before this version, so `Human` is the only shape a legacy
//!   payload could have carried. An explicit `"actor_kind": null` is treated
//!   the same as absent (no real producer emits it; pre-0.4.0 payloads lack
//!   the field and 0.4.0+ always writes a string). An **unknown** `actor_kind`
//!   value (anything other than `"human"` / `"service"`) is a hard
//!   deserialization error — it fails closed rather than guessing a default.

use br_core_kernel::{Actor, ServiceAccountId, UserId};
use serde::de::Deserializer;
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identity + correlation context attached to each domain event.
///
/// Construct via [`EventMetadata::new`] (then [`with_causation`] if a causing
/// event exists). Fields stay `pub` for read access; `#[non_exhaustive]` keeps
/// construction going through the constructors so a future field is an additive
/// change rather than a breaking one.
///
/// [`with_causation`]: EventMetadata::with_causation
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventMetadata {
    /// Who performed the action.
    pub actor: Actor,
    /// Correlates every event produced while handling one inbound request.
    pub correlation_id: Uuid,
    /// The event that directly caused this one, if any.
    pub causation_id: Option<Uuid>,
}

impl EventMetadata {
    /// New metadata with no causation. Add a cause with [`with_causation`].
    ///
    /// [`with_causation`]: EventMetadata::with_causation
    pub fn new(actor: Actor, correlation_id: Uuid) -> Self {
        Self {
            actor,
            correlation_id,
            causation_id: None,
        }
    }

    /// Set the causing event's id (builder-style; consumes and returns `self`).
    #[must_use]
    pub fn with_causation(mut self, causation_id: Uuid) -> Self {
        self.causation_id = Some(causation_id);
        self
    }
}

/// The `actor_kind` discriminant on the wire. Absent in pre-0.4.0 payloads.
///
/// Only `Deserialize` is derived: it is the input discriminant, and its
/// closed variant set is exactly what fails an unknown value. Serialization
/// writes the kind string directly in [`EventMetadata::serialize`].
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum ActorKindWire {
    Human,
    Service,
}

/// Private mirror of the flat wire object. `EventMetadata`'s manual
/// `Serialize`/`Deserialize` go through this so the flat shape and the
/// legacy-default policy live in one place.
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
        // `causation_id` is skipped when `None` to preserve the exact pre-0.4.0
        // shape for the common no-causation case (the only field count that
        // changes is the new, always-present `actor_kind`).
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
        // Absent `actor_kind` ⇒ legacy payload ⇒ Human (the only actor shape
        // that existed before machine actors entered this envelope). Unknown
        // values already failed in `ActorKindWire`'s derived `Deserialize`
        // (fail closed), so by here `actor_kind` is one of the two known
        // variants or `None`.
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

    // (a) serialize emits actor_id + actor_kind + correlation/causation.
    #[test]
    fn serialize_emits_flat_actor_id_and_kind() {
        let meta = EventMetadata::new(human(Uuid::from_u128(0xAB)), Uuid::from_u128(0xCD))
            .with_causation(Uuid::from_u128(0xEF));
        let json = serde_json::to_value(&meta).unwrap();
        assert_eq!(json["actor_id"], Uuid::from_u128(0xAB).to_string());
        assert_eq!(json["actor_kind"], "human");
        assert_eq!(json["correlation_id"], Uuid::from_u128(0xCD).to_string());
        assert_eq!(json["causation_id"], Uuid::from_u128(0xEF).to_string());
        // No nested actor object leaked onto the wire — it is flattened.
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

    // (b) a legacy payload (no actor_kind) deserializes to Actor::Human.
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

    // An explicit null actor_kind is treated as absent → Human (documented;
    // no real producer emits null — this pins the tolerated behavior).
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

    // (c) actor_kind "service" round-trips.
    #[test]
    fn service_actor_kind_roundtrips() {
        let meta = EventMetadata::new(service(Uuid::from_u128(0x55)), Uuid::nil());
        let json = serde_json::to_string(&meta).unwrap();
        let back: EventMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.actor, service(Uuid::from_u128(0x55)));
        assert!(back.actor.is_service());
    }

    // (d) an unknown actor_kind value fails closed.
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
        // The discriminant itself fails closed on anything outside the two
        // known variants — the reason `EventMetadata` never has to default an
        // unknown kind.
        assert!(serde_json::from_str::<ActorKindWire>("\"robot\"").is_err());
        assert!(serde_json::from_str::<ActorKindWire>("\"human\"").is_ok());
        assert!(serde_json::from_str::<ActorKindWire>("\"service\"").is_ok());
    }

    // (e) full round-trip, both variants.
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
