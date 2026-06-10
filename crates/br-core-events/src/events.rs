//! The event shapes themselves: [`RawEvent`] (pre-persistence, producer-side)
//! and [`DomainEvent`] (persisted / replayed).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// What an aggregate emits **before** persistence — no id, timestamp, or
/// metadata yet. In-process producer-side type; intentionally not
/// `Serialize`/`Deserialize`.
///
/// Construct via [`RawEvent::new`]. `#[non_exhaustive]` keeps construction
/// through the constructor so a future field stays additive.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RawEvent {
    pub aggregate_type: String,
    pub aggregate_id: Uuid,
    pub event_type: String,
    pub payload: serde_json::Value,
}

impl RawEvent {
    /// A raw event for `aggregate_id` of `aggregate_type`, of kind
    /// `event_type`, carrying `payload`.
    pub fn new(
        aggregate_type: impl Into<String>,
        aggregate_id: Uuid,
        event_type: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            aggregate_type: aggregate_type.into(),
            aggregate_id,
            event_type: event_type.into(),
            payload,
        }
    }
}

/// What the event store stores and replays.
///
/// `metadata` is a `serde_json::Value` (typically a serialized
/// [`EventMetadata`](crate::EventMetadata)) so the store stays agnostic to the
/// metadata's exact shape. Construct via [`DomainEvent::new`];
/// `#[non_exhaustive]` keeps construction through the constructor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct DomainEvent {
    pub id: Uuid,
    pub aggregate_id: Uuid,
    pub aggregate_type: String,
    pub event_type: String,
    pub payload: serde_json::Value,
    pub metadata: serde_json::Value,
    pub occurred_at: DateTime<Utc>,
}

impl DomainEvent {
    /// A persisted domain event. The caller supplies the event `id` (UUIDv7)
    /// and `occurred_at` so retries and replays stay idempotent and ordering
    /// is the producer's to decide.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: Uuid,
        aggregate_id: Uuid,
        aggregate_type: impl Into<String>,
        event_type: impl Into<String>,
        payload: serde_json::Value,
        metadata: serde_json::Value,
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

    #[test]
    fn raw_event_new_sets_fields() {
        let evt = RawEvent::new(
            "Organization",
            Uuid::nil(),
            "OrgCreated",
            serde_json::json!({"name": "Acme"}),
        );
        assert_eq!(evt.aggregate_type, "Organization");
        assert_eq!(evt.event_type, "OrgCreated");
        assert_eq!(evt.payload["name"], "Acme");
    }

    #[test]
    fn raw_event_clone() {
        let evt = RawEvent::new(
            "User",
            Uuid::nil(),
            "UserCreated",
            serde_json::json!({"name": "test"}),
        );
        let cloned = evt.clone();
        assert_eq!(cloned.aggregate_type, evt.aggregate_type);
        assert_eq!(cloned.event_type, evt.event_type);
    }

    #[test]
    fn domain_event_new_sets_fields() {
        let evt = DomainEvent::new(
            Uuid::nil(),
            Uuid::nil(),
            "User",
            "UserCreated",
            serde_json::json!({"email": "a@b.com"}),
            serde_json::json!({"actor_id": Uuid::nil()}),
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
            serde_json::json!({"actor_id": Uuid::nil()}),
            Utc::now(),
        );
        let json = serde_json::to_string(&evt).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_type, "UserCreated");
        assert_eq!(back.aggregate_type, "User");
    }

    #[test]
    fn domain_event_payload_and_metadata_survive_roundtrip() {
        let evt = DomainEvent::new(
            Uuid::nil(),
            Uuid::nil(),
            "Organization",
            "OrgCreated",
            serde_json::json!({"org_id": "some-uuid", "name": "Acme"}),
            serde_json::json!({"actor_id": "some-uuid", "correlation_id": "some-uuid"}),
            Utc::now(),
        );
        let json = serde_json::to_string(&evt).unwrap();
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.payload["org_id"], "some-uuid");
        assert_eq!(back.payload["name"], "Acme");
        assert_eq!(back.metadata["actor_id"], "some-uuid");
        assert_eq!(back.metadata["correlation_id"], "some-uuid");
    }
}
