use std::time::Duration;

use async_nats::jetstream::AckKind;
use async_nats::jetstream::Message;
use serde::de::DeserializeOwned;

use crate::classify::classify_ack_error;
use crate::error::{ConsumeErrorKind, FabricError};

pub struct Delivered<E> {
    message: Message,
    subject: String,
    delivered_count: Option<i64>,
    payload: Result<E, FabricError>,
}

impl<E: DeserializeOwned> Delivered<E> {
    pub(crate) fn decode(message: Message) -> Self {
        let subject = message.subject.as_str().to_string();
        let delivered_count = message.info().map(|info| info.delivered).ok();
        let payload = decode_payload::<E>(&subject, delivered_count, &message.payload);
        Self {
            message,
            subject,
            delivered_count,
            payload,
        }
    }
}

fn decode_payload<E: DeserializeOwned>(
    subject: &str,
    delivered_count: Option<i64>,
    bytes: &[u8],
) -> Result<E, FabricError> {
    match delivered_count {
        Some(_) => {
            serde_json::from_slice::<E>(bytes).map_err(|err| FabricError::decode(subject, &err))
        }
        None => Err(FabricError::consume(
            ConsumeErrorKind::NoDeliveryInfo,
            format!("delivery info absent on '{subject}'"),
        )),
    }
}

impl<E> Delivered<E> {
    pub fn subject(&self) -> &str {
        &self.subject
    }

    pub fn delivered_count(&self) -> Option<i64> {
        self.delivered_count
    }

    pub fn payload(&self) -> Result<&E, &FabricError> {
        self.payload.as_ref()
    }

    pub async fn ack(self) -> Result<(), FabricError> {
        self.apply(AckKind::Ack).await
    }

    pub async fn nak(self, delay: Option<Duration>) -> Result<(), FabricError> {
        self.apply(AckKind::Nak(delay)).await
    }

    pub async fn term(self) -> Result<(), FabricError> {
        self.apply(AckKind::Term).await
    }

    async fn apply(self, kind: AckKind) -> Result<(), FabricError> {
        self.message
            .ack_with(kind)
            .await
            .map_err(|err| FabricError::consume(classify_ack_error(err.as_ref()), err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize, Debug)]
    struct Body {
        label: String,
    }

    #[test]
    fn absent_delivery_info_is_observable_as_no_delivery_info_never_a_silent_count() {
        let err = decode_payload::<Body>(
            "integration.cmd.notifier.notification.deliver.v1",
            None,
            br#"{"label":"hi"}"#,
        )
        .unwrap_err();
        assert!(
            matches!(
                err,
                FabricError::Consume {
                    kind: ConsumeErrorKind::NoDeliveryInfo,
                    ..
                }
            ),
            "an absent delivery count must surface as NoDeliveryInfo, not a fabricated 1"
        );
    }

    #[test]
    fn present_delivery_info_decodes_the_body() {
        let body =
            decode_payload::<Body>("integration.cmd.x.y.z.v1", Some(1), br#"{"label":"hi"}"#)
                .unwrap();
        assert_eq!(body.label, "hi");
    }

    #[test]
    fn present_delivery_info_routes_a_malformed_body_to_decode() {
        let err =
            decode_payload::<Body>("integration.cmd.x.y.z.v1", Some(1), b"{ not json").unwrap_err();
        assert!(matches!(err, FabricError::Decode { .. }));
    }
}
