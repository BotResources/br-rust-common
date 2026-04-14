use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventMetadata {
    pub actor_id: Uuid,
    pub correlation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct RawEvent {
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DomainEvent {
    pub id: Uuid,
    pub aggregate_id: Uuid,
    pub aggregate_type: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub metadata: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn metadata_serde_with_causation() {
        let meta = EventMetadata {
            actor_id: Uuid::nil(),
            correlation_id: Uuid::nil(),
            causation_id: Some(Uuid::nil()),
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: EventMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.causation_id, Some(Uuid::nil()));
    }

    #[test]
    fn metadata_serde_without_causation_skips_field() {
        let meta = EventMetadata {
            actor_id: Uuid::nil(),
            correlation_id: Uuid::nil(),
            causation_id: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("causation_id"));
    }

    #[test]
    fn raw_event_fields_accessible() {
        let evt = RawEvent {
            aggregate_type: "Organization".to_string(),
            aggregate_id: Uuid::nil(),
            event_type: "OrgCreated".to_string(),
            payload: serde_json::json!({"name": "Acme"}),
        };
        assert_eq!(evt.aggregate_type, "Organization");
        assert_eq!(evt.event_type, "OrgCreated");
    }

    #[test]
    fn domain_event_serde_roundtrip() {
        let evt = DomainEvent {
            id: Uuid::nil(),
            aggregate_id: Uuid::nil(),
            aggregate_type: "User".to_string(),
            event_type: "UserCreated".to_string(),
            payload: serde_json::json!({"email": "a@b.com"}),
            metadata: serde_json::json!({"actor_id": Uuid::nil()}),
            occurred_at: Utc::now(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type, "UserCreated");
        assert_eq!(back.aggregate_type, "User");
    }

    #[test]
    fn metadata_serde_without_causation_roundtrip() {
        let meta = EventMetadata {
            actor_id: Uuid::nil(),
            correlation_id: Uuid::nil(),
            causation_id: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let back: EventMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.causation_id, None);
        assert_eq!(back.actor_id, Uuid::nil());
    }

    #[test]
    fn metadata_deserialize_missing_causation_field() {
        let json = r#"{"actor_id":"00000000-0000-0000-0000-000000000000","correlation_id":"00000000-0000-0000-0000-000000000000"}"#;
        let meta: EventMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(meta.causation_id, None);
    }

    #[test]
    fn metadata_clone_is_independent() {
        let meta = EventMetadata {
            actor_id: Uuid::nil(),
            correlation_id: Uuid::nil(),
            causation_id: Some(Uuid::nil()),
        };
        let cloned = meta.clone();
        assert_eq!(cloned.actor_id, meta.actor_id);
        assert_eq!(cloned.correlation_id, meta.correlation_id);
        assert_eq!(cloned.causation_id, meta.causation_id);
    }

    #[test]
    fn raw_event_clone() {
        let evt = RawEvent {
            aggregate_type: "User".to_string(),
            aggregate_id: Uuid::nil(),
            event_type: "UserCreated".to_string(),
            payload: serde_json::json!({"name": "test"}),
        };
        let cloned = evt.clone();
        assert_eq!(cloned.aggregate_type, evt.aggregate_type);
        assert_eq!(cloned.event_type, evt.event_type);
    }

    #[test]
    fn domain_event_payload_and_metadata_survive_roundtrip() {
        let evt = DomainEvent {
            id: Uuid::nil(),
            aggregate_id: Uuid::nil(),
            aggregate_type: "Organization".to_string(),
            event_type: "OrgCreated".to_string(),
            payload: serde_json::json!({"org_id": "some-uuid", "name": "Acme"}),
            metadata: serde_json::json!({"actor_id": "some-uuid", "correlation_id": "some-uuid"}),
            occurred_at: Utc::now(),
        };
        let json = serde_json::to_string(&evt).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payload["org_id"], "some-uuid");
        assert_eq!(back.payload["name"], "Acme");
        assert_eq!(back.metadata["actor_id"], "some-uuid");
        assert_eq!(back.metadata["correlation_id"], "some-uuid");
    }
}
