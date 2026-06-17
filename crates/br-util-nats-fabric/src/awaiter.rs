use std::time::Duration;

use async_nats::Subscriber;
use futures_util::StreamExt;
use futures_util::stream::SelectAll;
use serde::Deserialize;
use uuid::Uuid;

use br_core_integration::EventMetadata;

use crate::classify::classify_get_stream;
use crate::coords::{EventCoords, IntegrationSubject};
use crate::error::{ConsumeErrorKind, FabricError};
use crate::fabric::Fabric;
use crate::stream::INTEGRATION_EVT;

#[non_exhaustive]
pub struct CorrelatedMatch {
    pub subject: String,
    pub metadata: EventMetadata,
    pub payload: Vec<u8>,
}

#[derive(Deserialize)]
struct CorrelationProbe {
    metadata: EventMetadata,
}

pub struct CorrelatedAwaiter {
    messages: SelectAll<Subscriber>,
}

impl Fabric {
    pub async fn await_event(
        &self,
        coords: &EventCoords,
    ) -> Result<CorrelatedAwaiter, FabricError> {
        self.await_events(std::slice::from_ref(&coords)).await
    }

    pub async fn await_events(
        &self,
        coords: &[&EventCoords],
    ) -> Result<CorrelatedAwaiter, FabricError> {
        let jetstream = self.context();
        jetstream
            .get_stream(INTEGRATION_EVT)
            .await
            .map_err(|e| FabricError::consume(classify_get_stream(&e), e.to_string()))?;

        let mut messages = SelectAll::new();
        for coord in coords {
            let subscriber = jetstream
                .client()
                .subscribe(coord.subject())
                .await
                .map_err(|e| FabricError::consume(ConsumeErrorKind::Other, e.to_string()))?;
            messages.push(subscriber);
        }

        Ok(CorrelatedAwaiter { messages })
    }
}

impl CorrelatedAwaiter {
    pub async fn await_correlation(
        &mut self,
        correlation_id: Uuid,
        deadline: Duration,
    ) -> Result<Option<CorrelatedMatch>, FabricError> {
        let wait = tokio::time::sleep(deadline);
        tokio::pin!(wait);

        loop {
            tokio::select! {
                () = &mut wait => return Ok(None),
                next = self.messages.next() => {
                    let Some(message) = next else {
                        return Err(FabricError::consume(
                            ConsumeErrorKind::ConsumerGone,
                            "awaiter subscription ended (NATS connection closed)",
                        ));
                    };

                    let Ok(probe) = serde_json::from_slice::<CorrelationProbe>(&message.payload)
                    else {
                        continue;
                    };

                    if probe.metadata.correlation_id == correlation_id {
                        return Ok(Some(CorrelatedMatch {
                            subject: message.subject.as_str().to_string(),
                            metadata: probe.metadata,
                            payload: message.payload.to_vec(),
                        }));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_kernel::{Actor, UserId};

    fn envelope_bytes(correlation_id: Uuid) -> Vec<u8> {
        let metadata = EventMetadata::new(Actor::Human(UserId::from(Uuid::nil())), correlation_id);
        serde_json::to_vec(&serde_json::json!({
            "event_id": Uuid::nil(),
            "event_type": "user.created",
            "version": 1,
            "occurred_at": "2023-11-14T22:13:20Z",
            "metadata": serde_json::to_value(&metadata).unwrap(),
            "payload": { "anything": true },
        }))
        .unwrap()
    }

    #[test]
    fn probe_reads_correlation_id_ignoring_payload() {
        let c = Uuid::from_u128(0xC0FFEE);
        let bytes = envelope_bytes(c);
        let probe: CorrelationProbe = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(probe.metadata.correlation_id, c);
    }

    #[test]
    fn probe_decode_fails_on_non_envelope() {
        let bytes = serde_json::to_vec(&serde_json::json!({ "not": "an envelope" })).unwrap();
        assert!(serde_json::from_slice::<CorrelationProbe>(&bytes).is_err());
    }
}
