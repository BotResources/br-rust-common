use std::time::Duration;

use async_nats::jetstream::consumer::{self, pull::Config as PullConfig};
use futures_util::StreamExt;
use serde::Deserialize;
use uuid::Uuid;

use crate::awaiter_config::AwaiterConfig;
use crate::nats_classify::{classify_get_stream, classify_messages_error};
use crate::{ConsumeErrorKind, EventMetadata, IntegrationError};

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
    messages: consumer::pull::Stream,
}

impl CorrelatedAwaiter {
    pub async fn create(
        jetstream: &async_nats::jetstream::Context,
        stream_name: impl AsRef<str>,
        filter_subjects: Vec<String>,
    ) -> Result<Self, IntegrationError> {
        Self::create_with(
            jetstream,
            stream_name,
            filter_subjects,
            AwaiterConfig::default(),
        )
        .await
    }

    pub async fn create_with(
        jetstream: &async_nats::jetstream::Context,
        stream_name: impl AsRef<str>,
        filter_subjects: Vec<String>,
        config: AwaiterConfig,
    ) -> Result<Self, IntegrationError> {
        let stream = jetstream
            .get_stream(stream_name.as_ref())
            .await
            .map_err(|e| IntegrationError::consume(classify_get_stream(&e), e.to_string()))?;

        let config = PullConfig {
            durable_name: None,
            deliver_policy: consumer::DeliverPolicy::New,
            ack_policy: consumer::AckPolicy::None,
            filter_subjects,
            inactive_threshold: config.inactive_threshold,
            ..Default::default()
        };
        let consumer = stream
            .create_consumer(config)
            .await
            .map_err(|e| IntegrationError::consume(ConsumeErrorKind::Other, e.to_string()))?;
        let messages = consumer
            .messages()
            .await
            .map_err(|e| IntegrationError::consume(ConsumeErrorKind::Other, e.to_string()))?;

        Ok(Self { messages })
    }

    pub async fn await_correlation(
        &mut self,
        correlation_id: Uuid,
        deadline: Duration,
    ) -> Result<Option<CorrelatedMatch>, IntegrationError> {
        let wait = tokio::time::sleep(deadline);
        tokio::pin!(wait);

        loop {
            tokio::select! {
                () = &mut wait => return Ok(None),
                next = self.messages.next() => {
                    let Some(message) = next else {
                        return Err(IntegrationError::consume(
                            ConsumeErrorKind::ConsumerGone,
                            "awaiter pull stream ended (ephemeral consumer gone)",
                        ));
                    };
                    let message = message.map_err(|e| {
                        IntegrationError::consume(classify_messages_error(&e), e.to_string())
                    })?;

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
            "event_type": "service_scope.accepted",
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
