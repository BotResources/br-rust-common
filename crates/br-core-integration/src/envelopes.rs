use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub use br_core_events::EventMetadata;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[non_exhaustive]
pub struct IntegrationEvent<T> {
    pub event_id: Uuid,
    pub event_type: String,
    pub version: u8,
    pub occurred_at: DateTime<Utc>,
    pub metadata: EventMetadata,
    pub payload: T,
}

impl<T> IntegrationEvent<T> {
    pub fn new(
        event_id: Uuid,
        event_type: impl Into<String>,
        version: u8,
        occurred_at: DateTime<Utc>,
        metadata: EventMetadata,
        payload: T,
    ) -> Self {
        Self {
            event_id,
            event_type: event_type.into(),
            version,
            occurred_at,
            metadata,
            payload,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[non_exhaustive]
pub struct IntegrationCommand<T> {
    pub command_id: Uuid,
    pub command_type: String,
    pub version: u8,
    pub issued_at: DateTime<Utc>,
    pub metadata: EventMetadata,
    pub payload: T,
}

impl<T> IntegrationCommand<T> {
    pub fn new(
        command_id: Uuid,
        command_type: impl Into<String>,
        version: u8,
        issued_at: DateTime<Utc>,
        metadata: EventMetadata,
        payload: T,
    ) -> Self {
        Self {
            command_id,
            command_type: command_type.into(),
            version,
            issued_at,
            metadata,
            payload,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_kernel::{Actor, ServiceAccountId, UserId};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    struct TestPayload {
        name: String,
        count: u32,
    }

    fn sample_metadata() -> EventMetadata {
        EventMetadata::new(Actor::Human(UserId::from(Uuid::nil())), Uuid::nil())
            .with_causation(Uuid::nil())
    }

    fn sample_event() -> IntegrationEvent<TestPayload> {
        IntegrationEvent::new(
            Uuid::nil(),
            "user.created",
            1,
            DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            sample_metadata(),
            TestPayload {
                name: "alice".to_string(),
                count: 7,
            },
        )
    }

    fn sample_command() -> IntegrationCommand<TestPayload> {
        IntegrationCommand::new(
            Uuid::nil(),
            "notification.send",
            2,
            DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            sample_metadata(),
            TestPayload {
                name: "bob".to_string(),
                count: 3,
            },
        )
    }

    #[test]
    fn event_roundtrip() {
        let evt = sample_event();
        let json = serde_json::to_string(&evt).unwrap();
        let back: IntegrationEvent<TestPayload> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.event_id, evt.event_id);
        assert_eq!(back.event_type, evt.event_type);
        assert_eq!(back.version, evt.version);
        assert_eq!(back.occurred_at, evt.occurred_at);
        assert_eq!(back.payload, evt.payload);
    }

    #[test]
    fn command_roundtrip() {
        let cmd = sample_command();
        let json = serde_json::to_string(&cmd).unwrap();
        let back: IntegrationCommand<TestPayload> = serde_json::from_str(&json).unwrap();
        assert_eq!(back.command_id, cmd.command_id);
        assert_eq!(back.command_type, cmd.command_type);
        assert_eq!(back.version, cmd.version);
        assert_eq!(back.issued_at, cmd.issued_at);
        assert_eq!(back.payload, cmd.payload);
    }

    #[test]
    fn legacy_metadata_in_event_defaults_to_human() {
        let legacy = r#"{
            "event_id":"00000000-0000-0000-0000-000000000000",
            "event_type":"user.created",
            "version":1,
            "occurred_at":"2023-11-14T22:13:20Z",
            "metadata":{
                "actor_id":"00000000-0000-0000-0000-0000000000ab",
                "correlation_id":"00000000-0000-0000-0000-0000000000cd"
            },
            "payload":{"name":"alice","count":7}
        }"#;
        let evt: IntegrationEvent<TestPayload> = serde_json::from_str(legacy).unwrap();
        assert!(evt.metadata.actor.is_human());
        assert_eq!(evt.metadata.actor.id(), Uuid::from_u128(0xAB));
    }

    #[test]
    fn service_metadata_in_event_roundtrips() {
        let meta = EventMetadata::new(
            Actor::Service(ServiceAccountId::from(Uuid::from_u128(9))),
            Uuid::nil(),
        );
        let evt = IntegrationEvent::new(
            Uuid::nil(),
            "svc.acted",
            1,
            DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            meta,
            TestPayload {
                name: "svc".to_string(),
                count: 1,
            },
        );
        let json = serde_json::to_string(&evt).unwrap();
        let back: IntegrationEvent<TestPayload> = serde_json::from_str(&json).unwrap();
        assert!(back.metadata.actor.is_service());
    }
}
