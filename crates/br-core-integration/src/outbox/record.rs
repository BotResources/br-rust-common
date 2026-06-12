//! The staged outbox row as a typed value — what `stage` writes and the relay
//! reads. Pure (no I/O, no sqlx): the store maps it to/from a database row.

use uuid::Uuid;

use crate::outbox::OutboxStatus;

/// A message staged for transactional publish.
///
/// One row per integration message, written in the **same transaction** as the
/// domain state it announces, so a crash between the domain commit and the bus
/// publish cannot lose it: the relay finds the `Pending` row on recovery and
/// publishes it.
///
/// `id` is the outbox row id — a creator-supplied **UUIDv7** so a re-`stage`
/// after a retried request is idempotent (`ON CONFLICT (id) DO NOTHING`) rather
/// than inserting a duplicate. It is *not* the message's own `event_id` /
/// `command_id`; those live inside `payload`.
///
/// `subject` is the fully-built integration subject (use
/// [`integration_subject`](crate::integration_subject)); `payload` is the
/// already-serialized envelope JSON the relay publishes verbatim.
///
/// `#[non_exhaustive]`: build via [`OutboxRecord::stage`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OutboxRecord {
    /// Outbox row id (UUIDv7, creator-supplied) — the idempotency key for the
    /// stage insert. Not the message's own id.
    pub id: Uuid,
    /// The subject the relay publishes on.
    pub subject: String,
    /// The serialized envelope JSON, published verbatim.
    pub payload: serde_json::Value,
    /// Current lifecycle state.
    pub status: OutboxStatus,
    /// Publish attempts the relay has recorded so far. `0` when freshly staged.
    pub attempts: u32,
}

impl OutboxRecord {
    /// A freshly staged record: [`OutboxStatus::Pending`], zero attempts. The
    /// caller supplies the UUIDv7 `id`, the built `subject`, and the serialized
    /// envelope `payload`.
    ///
    /// Prefer [`stage_event`](Self::stage_event) / [`stage_command`](Self::stage_command)
    /// to serialize a typed envelope without touching `serde_json` at the call
    /// site.
    pub fn stage(id: Uuid, subject: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            id,
            subject: subject.into(),
            payload,
            status: OutboxStatus::Pending,
            attempts: 0,
        }
    }

    /// Stage a typed [`IntegrationEvent`](crate::IntegrationEvent), serializing
    /// its envelope to the stored `payload`. Returns
    /// [`IntegrationError::Serialization`](crate::IntegrationError::Serialization)
    /// if encoding fails (before anything is written).
    pub fn stage_event<T: serde::Serialize>(
        id: Uuid,
        subject: impl Into<String>,
        event: &crate::IntegrationEvent<T>,
    ) -> Result<Self, crate::IntegrationError> {
        Ok(Self::stage(id, subject, serde_json::to_value(event)?))
    }

    /// Stage a typed [`IntegrationCommand`](crate::IntegrationCommand),
    /// serializing its envelope to the stored `payload`. See
    /// [`stage_event`](Self::stage_event).
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

    // GIVEN a stage call WHEN the record is built THEN it is Pending with zero attempts
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

    // GIVEN a typed event WHEN staged THEN the payload is the serialized envelope
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
        // The stored payload round-trips back to the same envelope.
        let back: IntegrationEvent<UserCreatedV1> = serde_json::from_value(rec.payload).unwrap();
        assert_eq!(back.event_type, "user.created");
        assert_eq!(back.payload.user_id, Uuid::nil());
    }
}
