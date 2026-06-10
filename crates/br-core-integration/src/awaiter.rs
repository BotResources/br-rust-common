//! The **awaiter shape**: a per-replica, per-boot ephemeral consumer that
//! resolves when a delivered message's `correlation_id` matches the awaited
//! value, ignoring every other message.
//!
//! This is the shape a declaring replica uses to *await a correlated reply* —
//! e.g. a service that published `identity.cmd.service_scope.declare.v1` with
//! `correlation_id = C` and now awaits its confirmation on
//! `identity.evt.service_scope.{accepted,rejected}.v1`. It filters the
//! event subject(s) for the one message carrying `C` and ignores everything
//! else (other replicas' confirmations, unrelated events).
//!
//! ## Ephemeral, not infrastructure
//!
//! The consumer is **ephemeral** (no durable name) and **per-boot**: each
//! replica creates its own at startup with a fresh `correlation_id`. Creating
//! an ephemeral consumer at runtime is a read cursor, not provisioning, so it
//! does **not** violate the never-auto-provision rule. The **stream**, by
//! contrast, must pre-exist: a missing stream is a fail-loud
//! [`IntegrationError::Consume`] with kind `NoStream` — the awaiter never
//! creates it.
//!
//! No queue-group: every replica must *see* all confirmations to filter its
//! own, so each runs an independent consumer (the opposite of the durable
//! wrapper's shared delivery).
//!
//! ## Subscribe-first contract (and what is missed by design)
//!
//! The deliver policy is `DeliverPolicy::New`: the consumer only sees
//! messages the server receives **after** it is created. A confirmation emitted
//! *before* the awaiter exists is **missed by design**. The caller's protocol
//! makes this safe: create the awaiter **first**, *then* publish the command
//! (subscribe-first); and on a wait timeout, re-publish the command (same `C`)
//! and keep waiting — the awaiter stays armed across waits (up to the configured
//! `inactive_threshold`; see below), so the re-published confirmation is caught
//! with no gap. Duplicate confirmations are expected and harmless: the first
//! correlated match wins and later ones are simply never read (or ignored on a
//! subsequent wait).
//!
//! ## Stays armed only up to `inactive_threshold`
//!
//! The ephemeral consumer is created with an explicit `inactive_threshold`
//! (default [`AwaiterConfig::DEFAULT_INACTIVE_THRESHOLD`], 300s). *During* a
//! [`await_correlation`](CorrelatedAwaiter::await_correlation) the pull stream
//! issues pull requests that keep the consumer alive; but *between* waits — after
//! an `Ok(None)`, while the caller backs off and re-publishes — nothing polls it,
//! so no pull requests issue. The server reaps an ephemeral consumer after
//! `inactive_threshold` of such inactivity. The awaiter therefore stays armed
//! across waits **up to `inactive_threshold` of inactivity**; beyond that the
//! server deletes the consumer and the next
//! [`await_correlation`](CorrelatedAwaiter::await_correlation) **fails loud**
//! with [`ConsumeErrorKind::ConsumerGone`]
//! (rather than silently missing the reply on a recreated `New`-policy consumer).
//! Set the threshold via [`CorrelatedAwaiter::create_with`] generously above the
//! longest expected gap between a timed-out wait and the next re-publish.

use std::time::Duration;

use async_nats::jetstream::consumer::{self, pull::Config as PullConfig};
use futures_util::StreamExt;
use serde::Deserialize;
use uuid::Uuid;

use crate::awaiter_config::AwaiterConfig;
use crate::nats_classify::{classify_get_stream, classify_messages_error};
use crate::{ConsumeErrorKind, IntegrationError, MessageMetadata};

/// A correlated message matched by the awaiter. The caller decides which typed
/// payload to decode from `payload` based on which `subject` matched (the two
/// confirmation subjects carry two different payload types).
#[non_exhaustive]
pub struct CorrelatedMatch {
    /// The subject the matched message arrived on — lets the caller pick the
    /// right payload type to decode (e.g. `accepted` vs `rejected`).
    pub subject: String,
    /// The full message metadata, including the matched `correlation_id`.
    pub metadata: MessageMetadata,
    /// The raw message payload bytes. The caller deserializes the typed
    /// envelope appropriate to `subject`.
    pub payload: Vec<u8>,
}

/// Minimal probe to read `correlation_id` off any integration envelope without
/// knowing its payload type: serde ignores the unmodelled `payload` and the
/// other envelope fields, decoding only the metadata.
#[derive(Deserialize)]
struct CorrelationProbe {
    metadata: MessageMetadata,
}

/// A per-boot ephemeral consumer that resolves on a matching `correlation_id`.
/// See the [module docs](crate::awaiter) for the subscribe-first contract and
/// the ephemeral-not-infrastructure rationale.
pub struct CorrelatedAwaiter {
    messages: consumer::pull::Stream,
}

impl CorrelatedAwaiter {
    /// Create the ephemeral awaiter over one or more `filter_subjects` on a
    /// **pre-declared** stream, with the default [`AwaiterConfig`] (300s
    /// `inactive_threshold`). Fails loud if the stream is missing
    /// ([`ConsumeErrorKind::NoStream`]). The ephemeral consumer itself is
    /// created here — a read cursor, not provisioning.
    ///
    /// `filter_subjects` may be concrete subjects or NATS wildcards; pass the
    /// two confirmation subjects (e.g. `identity.evt.service_scope.accepted.v1`
    /// and `…rejected.v1`) to await either on one consumer.
    ///
    /// The deliver policy is `DeliverPolicy::New`: only messages received
    /// after creation are seen. Create the awaiter **before** publishing the
    /// command it awaits a reply to (see the module docs).
    ///
    /// Use [`create_with`](Self::create_with) to override the
    /// `inactive_threshold` for a workload whose gap between waits can exceed
    /// the default.
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

    /// Create the ephemeral awaiter with an explicit [`AwaiterConfig`]. Identical
    /// to [`create`](Self::create) but lets the caller set the
    /// `inactive_threshold` — raise it above the longest expected idle gap
    /// between a timed-out wait and the next re-publish, or the server reaps the
    /// consumer mid-protocol (see the [module docs](crate::awaiter)).
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

        // Ephemeral (no durable name), explicit `New` so we only see replies
        // emitted after we subscribed, `AckPolicy::None` because this is a read
        // cursor — we advance through messages, we do not track work. The
        // explicit `inactive_threshold` is load-bearing: `..Default::default()`
        // leaves it `Duration::ZERO`, which serde skips, so the server applies
        // ITS short ephemeral default and reaps us between waits (see module
        // docs). Setting it keeps the awaiter armed across the re-publish gap.
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

    /// Wait up to `deadline` for a message whose `correlation_id` equals
    /// `correlation_id`, ignoring every other message that arrives in the
    /// meantime.
    ///
    /// - `Ok(Some(match))` — a correlated message arrived; returns its subject,
    ///   metadata, and raw payload for the caller to decode.
    /// - `Ok(None)` — the deadline elapsed with no match. The awaiter stays
    ///   **armed**: call again (after re-publishing, same `correlation_id`) to
    ///   keep waiting from where it left off, with no missed messages in
    ///   between.
    /// - `Err(_)` — a transport failure or the consumer ended.
    ///
    /// A message that fails the `correlation_id` probe decode is skipped (it is
    /// some other producer's malformed envelope, not ours to term — this is a
    /// read cursor, not the durable wrapper); the wait continues until the
    /// deadline.
    pub async fn await_correlation(
        &mut self,
        correlation_id: Uuid,
        deadline: Duration,
    ) -> Result<Option<CorrelatedMatch>, IntegrationError> {
        let wait = tokio::time::sleep(deadline);
        tokio::pin!(wait);

        loop {
            tokio::select! {
                // Deadline elapsed: stay armed, report no match.
                () = &mut wait => return Ok(None),
                next = self.messages.next() => {
                    let Some(message) = next else {
                        // The pull stream ended with no error to classify. For an
                        // ephemeral consumer this is the consumer being gone
                        // (reaped past `inactive_threshold`, or its backing task
                        // closed): fail loud as `ConsumerGone` rather than spin.
                        return Err(IntegrationError::consume(
                            ConsumeErrorKind::ConsumerGone,
                            "awaiter pull stream ended (ephemeral consumer gone)",
                        ));
                    };
                    // A consumer-gone kind (`ConsumerDeleted` / `MissingHeartbeat`
                    // / `NoResponders` — the latter is what a reaped ephemeral
                    // consumer actually yields on the next pull) is classified as
                    // `ConsumerGone`; preserve the source text in `detail` rather
                    // than discarding it behind a fixed string.
                    let message = message.map_err(|e| {
                        IntegrationError::consume(classify_messages_error(&e), e.to_string())
                    })?;

                    // Read only the correlation_id; ignore the payload type.
                    let Ok(probe) = serde_json::from_slice::<CorrelationProbe>(&message.payload)
                    else {
                        // Not a recognisable integration envelope — not ours to
                        // judge. Skip and keep waiting.
                        continue;
                    };

                    if probe.metadata.correlation_id == correlation_id {
                        return Ok(Some(CorrelatedMatch {
                            subject: message.subject.as_str().to_string(),
                            metadata: probe.metadata,
                            payload: message.payload.to_vec(),
                        }));
                    }
                    // Uncorrelated (another replica's confirmation, an unrelated
                    // event): ignore and keep waiting.
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
        let metadata =
            MessageMetadata::new(Actor::Human(UserId::from(Uuid::nil())), correlation_id);
        // Shape a full integration-event envelope; the probe must read only the
        // metadata and ignore event_id / payload / the rest.
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
