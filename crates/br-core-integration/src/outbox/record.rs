use uuid::Uuid;

use crate::outbox::OutboxStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutboxRecord {
    pub id: Uuid,
    pub subject: String,
    pub payload: serde_json::Value,
    pub status: OutboxStatus,
    pub attempts: u32,
}

impl OutboxRecord {
    pub fn stage(id: Uuid, subject: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id,
            subject: subject.into(),
            payload,
            status: OutboxStatus::Pending,
            attempts: 0,
        }
    }

    pub fn stage_event<T: serde::Serialize>(
        id: Uuid,
        subject: impl Into<String>,
        event: &crate::IntegrationEvent<T>,
    ) -> Result<Self, crate::IntegrationError> {
        Ok(Self::stage(id, subject, serde_json::to_value(event)?))
    }

    pub fn stage_command<T: serde::Serialize>(
        id: Uuid,
        subject: impl Into<String>,
        command: &crate::IntegrationCommand<T>,
    ) -> Result<Self, crate::IntegrationError> {
        Ok(Self::stage(id, subject, serde_json::to_value(command)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Actor, IntegrationEvent, MessageMetadata, UserId};
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    struct UserCreatedV1 {
        user_id: Uuid,
    }

    #[test]
    fn freshly_staged_is_pending_with_zero_attempts() {
        let rec = OutboxRecord::stage(
            Uuid::nil(),
            "identity.evt.user.created.v1",
            serde_json::json!({"k": "v"}),
        );
        assert_eq!(rec.status, OutboxStatus::Pending);
        assert_eq!(rec.attempts, 0);
        assert_eq!(rec.subject, "identity.evt.user.created.v1");
    }

    #[test]
    fn stage_event_serializes_the_envelope() {
        let evt = IntegrationEvent::new(
            Uuid::nil(),
            "user.created",
            1,
            DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            MessageMetadata::new(Actor::Human(UserId::from(Uuid::nil())), Uuid::nil()),
            UserCreatedV1 {
                user_id: Uuid::nil(),
            },
        );
        let rec =
            OutboxRecord::stage_event(Uuid::nil(), "identity.evt.user.created.v1", &evt).unwrap();
        let back: IntegrationEvent<UserCreatedV1> = serde_json::from_value(rec.payload).unwrap();
        assert_eq!(back.event_type, "user.created");
        assert_eq!(back.payload.user_id, Uuid::nil());
    }
}
