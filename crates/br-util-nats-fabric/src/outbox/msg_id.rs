use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MessageIdSource {
    Envelope,
    Row,
}

impl MessageIdSource {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Envelope => "envelope",
            Self::Row => "row",
        }
    }
}

pub(super) fn message_id_for(row_id: Uuid, payload: &serde_json::Value) -> (Uuid, MessageIdSource) {
    match payload.get("event_id").and_then(serde_json::Value::as_str) {
        Some(raw) => match Uuid::parse_str(raw) {
            Ok(event_id) => (event_id, MessageIdSource::Envelope),
            Err(_) => (row_id, MessageIdSource::Row),
        },
        None => (row_id, MessageIdSource::Row),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row_id() -> Uuid {
        Uuid::parse_str("00000000-0000-7000-8000-0000000000ff").unwrap()
    }

    #[test]
    fn an_envelope_payload_dedups_on_its_event_id() {
        let event_id = Uuid::now_v7();
        let payload = serde_json::json!({
            "event_id": event_id.to_string(),
            "event_type": "user.created",
            "payload": { "user_id": Uuid::nil().to_string() },
        });
        assert_eq!(
            message_id_for(row_id(), &payload),
            (event_id, MessageIdSource::Envelope)
        );
    }

    #[test]
    fn a_payload_without_an_envelope_id_falls_back_to_the_row_id() {
        let payload = serde_json::json!({ "anything": "at all" });
        assert_eq!(
            message_id_for(row_id(), &payload),
            (row_id(), MessageIdSource::Row)
        );
    }

    #[test]
    fn a_non_uuid_event_id_falls_back_to_the_row_id() {
        let payload = serde_json::json!({ "event_id": "not-a-uuid" });
        assert_eq!(
            message_id_for(row_id(), &payload),
            (row_id(), MessageIdSource::Row)
        );
    }

    #[test]
    fn a_non_string_event_id_falls_back_to_the_row_id() {
        let payload = serde_json::json!({ "event_id": 7 });
        assert_eq!(
            message_id_for(row_id(), &payload),
            (row_id(), MessageIdSource::Row)
        );
    }

    #[test]
    fn a_non_object_payload_falls_back_to_the_row_id() {
        let payload = serde_json::json!(["event_id", "spoofed"]);
        assert_eq!(
            message_id_for(row_id(), &payload),
            (row_id(), MessageIdSource::Row)
        );
    }
}
