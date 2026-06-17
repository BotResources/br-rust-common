use uuid::Uuid;

use br_core_integration::{IntegrationEvent, OutboxStatus};

use crate::coords::EventCoords;
use crate::error::FabricError;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutboxRecord {
    pub id: Uuid,
    pub destination: EventCoords,
    pub payload: serde_json::Value,
    pub status: OutboxStatus,
    pub attempts: u32,
}

impl OutboxRecord {
    pub fn stage(id: Uuid, destination: EventCoords, payload: serde_json::Value) -> Self {
        Self {
            id,
            destination,
            payload,
            status: OutboxStatus::Pending,
            attempts: 0,
        }
    }

    pub fn stage_event<T: serde::Serialize>(
        id: Uuid,
        destination: EventCoords,
        event: &IntegrationEvent<T>,
    ) -> Result<Self, FabricError> {
        Ok(Self::stage(id, destination, serde_json::to_value(event)?))
    }

    #[cfg(feature = "outbox")]
    pub(crate) fn subject(&self) -> String {
        use crate::coords::IntegrationSubject;
        self.destination.subject()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coords::{Aggregate, Bc, PastFact};
    use br_core_kernel::{Actor, UserId};
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    struct UserCreatedV1 {
        user_id: Uuid,
    }

    fn coords() -> EventCoords {
        EventCoords {
            producer: Bc::new("identity").unwrap(),
            aggregate: Aggregate::new("user").unwrap(),
            fact: PastFact::new("created").unwrap(),
            version: 1,
        }
    }

    #[test]
    fn freshly_staged_is_pending_with_zero_attempts() {
        let rec = OutboxRecord::stage(Uuid::nil(), coords(), serde_json::json!({"k": "v"}));
        assert_eq!(rec.status, OutboxStatus::Pending);
        assert_eq!(rec.attempts, 0);
    }

    #[cfg(feature = "outbox")]
    #[test]
    fn destination_renders_the_typed_subject() {
        let rec = OutboxRecord::stage(Uuid::nil(), coords(), serde_json::json!({}));
        assert_eq!(rec.subject(), "integration.evt.identity.user.created.v1");
    }

    #[test]
    fn stage_event_serializes_the_envelope() {
        use br_core_integration::EventMetadata;
        let evt = IntegrationEvent::new(
            Uuid::nil(),
            "user.created",
            1,
            DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            EventMetadata::new(Actor::Human(UserId::from(Uuid::nil())), Uuid::nil()),
            UserCreatedV1 {
                user_id: Uuid::nil(),
            },
        );
        let rec = OutboxRecord::stage_event(Uuid::nil(), coords(), &evt).unwrap();
        let back: IntegrationEvent<UserCreatedV1> = serde_json::from_value(rec.payload).unwrap();
        assert_eq!(back.event_type, "user.created");
    }
}
