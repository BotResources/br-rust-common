use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::metadata::EventMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DomainEvent {
    pub id: Uuid,
    pub aggregate_id: Uuid,
    pub aggregate_type: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub metadata: EventMetadata,
    pub occurred_at: DateTime<Utc>,
}

impl DomainEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        aggregate_id: Uuid,
        aggregate_type: impl Into<String>,
        event_type: impl Into<String>,
        payload: serde_json::Value,
        metadata: EventMetadata,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            aggregate_id,
            aggregate_type: aggregate_type.into(),
            event_type: event_type.into(),
            payload,
            metadata,
            occurred_at,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_kernel::{Actor, UserId};

    fn metadata() -> EventMetadata {
        EventMetadata::new(Actor::Human(UserId::from(Uuid::nil())), Uuid::nil())
    }

    #[test]
    fn domain_event_new_sets_fields() {
        let evt = DomainEvent::new(
            Uuid::nil(),
            Uuid::nil(),
            "User",
            "UserCreated",
            serde_json::json!({"email": "a@b.com"}),
            metadata(),
            Utc::now(),
        );
        assert_eq!(evt.aggregate_type, "User");
        assert_eq!(evt.event_type, "UserCreated");
    }

    #[test]
    fn domain_event_serde_roundtrip() {
        let evt = DomainEvent::new(
            Uuid::nil(),
            Uuid::nil(),
            "User",
            "UserCreated",
            serde_json::json!({"email": "a@b.com"}),
            metadata(),
            Utc::now(),
        );
        let json = serde_json::to_string(&evt).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type, "UserCreated");
        assert_eq!(back.aggregate_type, "User");
    }

    #[test]
    fn domain_event_payload_and_metadata_survive_roundtrip() {
        let meta = EventMetadata::new(
            Actor::Human(UserId::from(Uuid::from_u128(1))),
            Uuid::from_u128(2),
        );
        let evt = DomainEvent::new(
            Uuid::nil(),
            Uuid::nil(),
            "Organization",
            "OrgCreated",
            serde_json::json!({"org_id": "some-uuid", "name": "Acme"}),
            meta,
            Utc::now(),
        );
        let json = serde_json::to_string(&evt).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payload["org_id"], "some-uuid");
        assert_eq!(back.payload["name"], "Acme");
        assert_eq!(back.metadata.actor.id(), Uuid::from_u128(1));
        assert_eq!(back.metadata.correlation_id, Uuid::from_u128(2));
    }
}
