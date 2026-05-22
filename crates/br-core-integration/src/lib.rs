//! Typed envelopes and publisher trait for cross-bounded-context (integration)
//! messaging.
//!
//! Where [`br-core-events`] holds the shapes that travel *inside* a bounded
//! context's event store, this crate holds the shapes that travel *between*
//! contexts on the message bus.
//!
//! ## Types
//!
//! - [`MessageMetadata`] — actor / correlation / causation. Shaped like
//!   `EventMetadata` for now, kept as its own type because it may diverge
//!   (e.g., `actor_id` becoming an `Actor` enum to distinguish humans from
//!   service accounts).
//! - [`IntegrationEvent<T>`] — fact published by an emitting context.
//! - [`IntegrationCommand<T>`] — request asking a receiving context to act.
//!
//! ## Publishing
//!
//! [`IntegrationPublisher`] is **object-safe** so applications can hold an
//! `Arc<dyn IntegrationPublisher>`. The [`IntegrationPublisherExt`] blanket
//! provides typed helpers ([`publish_event`](IntegrationPublisherExt::publish_event),
//! [`publish_command`](IntegrationPublisherExt::publish_command), and their
//! `_if_connected` fire-and-forget counterparts).
//!
//! ## Implementations bundled here
//!
//! - [`NatsIntegrationPublisher`] — JetStream publisher; awaits the delivery
//!   ack on [`publish`](IntegrationPublisher::publish); logs and swallows errors
//!   on [`publish_if_connected`](IntegrationPublisher::publish_if_connected).
//! - [`NoopIntegrationPublisher`] — for tests.
//!
//! ## Subject naming convention
//!
//! Not enforced by the type system, but recommended:
//!
//! - Events:   `{bc}.evt.{aggregate}.{event_name}.v{N}`
//!   — e.g. `identity.evt.user.created.v1`
//! - Commands: `{bc}.cmd.{aggregate}.{command_name}.v{N}`
//!   — e.g. `notifier.cmd.notification.send.v1`
//!
//! Subscribers use NATS wildcards (`identity.evt.>`, `notifier.cmd.>`) to
//! consume relevant streams.
//!
//! [`br-core-events`]: https://github.com/BotResources/br-rust-common/tree/main/crates/br-core-events

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Identity / correlation context attached to every integration message.
///
/// Field semantics match `br_core_events::EventMetadata` for now; this is a
/// distinct type so it can diverge later (e.g., `actor_id` becoming an `Actor`
/// enum to separate human users from service accounts).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MessageMetadata {
    pub actor_id: Uuid,
    pub correlation_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub causation_id: Option<Uuid>,
}

/// Fact emitted by a bounded context. `payload` carries the context-specific
/// event shape.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IntegrationEvent<T> {
    pub event_id: Uuid,
    pub event_type: String,
    pub version: u8,
    pub occurred_at: DateTime<Utc>,
    pub metadata: MessageMetadata,
    pub payload: T,
}

/// Request asking a bounded context to perform an action. `payload` carries
/// the context-specific command shape.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct IntegrationCommand<T> {
    pub command_id: Uuid,
    pub command_type: String,
    pub version: u8,
    pub issued_at: DateTime<Utc>,
    pub metadata: MessageMetadata,
    pub payload: T,
}

/// Errors returned by [`IntegrationPublisher::publish`] and the typed helpers.
#[derive(thiserror::Error, Debug)]
pub enum IntegrationError {
    #[error("publish failed: {0}")]
    Publish(String),
    #[error("serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
}

/// Object-safe publishing interface.
///
/// Hold as `Arc<dyn IntegrationPublisher>` from application code. For typed
/// publishing, bring [`IntegrationPublisherExt`] into scope.
#[async_trait::async_trait]
pub trait IntegrationPublisher: Send + Sync {
    /// Publish raw JSON to `subject` and wait for the broker ack.
    async fn publish(
        &self,
        subject: &str,
        payload: serde_json::Value,
    ) -> Result<(), IntegrationError>;

    /// Fire-and-forget publish: never returns an error. Implementations should
    /// log failures (e.g., the broker is down) but never propagate them.
    async fn publish_if_connected(&self, subject: &str, payload: serde_json::Value);
}

/// Typed helpers built on top of [`IntegrationPublisher`].
///
/// Auto-implemented for every `IntegrationPublisher` (including
/// `dyn IntegrationPublisher`) via the blanket `impl` at the bottom of this
/// file. Not object-safe itself — the typed methods are monomorphized.
#[async_trait::async_trait]
pub trait IntegrationPublisherExt: IntegrationPublisher {
    async fn publish_event<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        event: &IntegrationEvent<T>,
    ) -> Result<(), IntegrationError> {
        let value = serde_json::to_value(event)?;
        self.publish(subject, value).await
    }

    async fn publish_command<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        command: &IntegrationCommand<T>,
    ) -> Result<(), IntegrationError> {
        let value = serde_json::to_value(command)?;
        self.publish(subject, value).await
    }

    async fn publish_event_if_connected<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        event: &IntegrationEvent<T>,
    ) {
        match serde_json::to_value(event) {
            Ok(value) => self.publish_if_connected(subject, value).await,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    subject = subject,
                    "integration event serialization failed; dropping"
                );
            }
        }
    }

    async fn publish_command_if_connected<T: serde::Serialize + Send + Sync>(
        &self,
        subject: &str,
        command: &IntegrationCommand<T>,
    ) {
        match serde_json::to_value(command) {
            Ok(value) => self.publish_if_connected(subject, value).await,
            Err(err) => {
                tracing::warn!(
                    error = %err,
                    subject = subject,
                    "integration command serialization failed; dropping"
                );
            }
        }
    }
}

impl<P: IntegrationPublisher + ?Sized> IntegrationPublisherExt for P {}

/// JetStream-backed publisher. Wraps an [`async_nats::jetstream::Context`].
///
/// - [`publish`](IntegrationPublisher::publish) serializes the JSON value to
///   bytes, sends it on `subject`, and awaits the broker ack. Any ack error
///   (broker down, no-responders, etc.) surfaces as
///   [`IntegrationError::Publish`].
/// - [`publish_if_connected`](IntegrationPublisher::publish_if_connected)
///   does the same but logs and swallows any error — useful for best-effort
///   side-channel emissions where the request handler must not fail because
///   the bus is unavailable.
pub struct NatsIntegrationPublisher {
    jetstream: async_nats::jetstream::Context,
}

impl NatsIntegrationPublisher {
    pub fn new(jetstream: async_nats::jetstream::Context) -> Self {
        Self { jetstream }
    }
}

#[async_trait::async_trait]
impl IntegrationPublisher for NatsIntegrationPublisher {
    async fn publish(
        &self,
        subject: &str,
        payload: serde_json::Value,
    ) -> Result<(), IntegrationError> {
        let bytes = serde_json::to_vec(&payload)?;
        let ack = self
            .jetstream
            .publish(subject.to_string(), bytes.into())
            .await
            .map_err(|e| IntegrationError::Publish(e.to_string()))?;
        ack.await
            .map_err(|e| IntegrationError::Publish(e.to_string()))?;
        Ok(())
    }

    async fn publish_if_connected(&self, subject: &str, payload: serde_json::Value) {
        if let Err(err) = self.publish(subject, payload).await {
            tracing::warn!(
                error = %err,
                subject = subject,
                "integration publish failed; dropping"
            );
        }
    }
}

/// No-op publisher. [`publish`](IntegrationPublisher::publish) always returns
/// `Ok(())`; [`publish_if_connected`](IntegrationPublisher::publish_if_connected)
/// does nothing. For tests and as a default when integration messaging is
/// disabled.
#[derive(Default, Debug, Clone, Copy)]
pub struct NoopIntegrationPublisher;

#[async_trait::async_trait]
impl IntegrationPublisher for NoopIntegrationPublisher {
    async fn publish(
        &self,
        _subject: &str,
        _payload: serde_json::Value,
    ) -> Result<(), IntegrationError> {
        Ok(())
    }

    async fn publish_if_connected(&self, _subject: &str, _payload: serde_json::Value) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    struct TestPayload {
        name: String,
        count: u32,
    }

    fn sample_metadata() -> MessageMetadata {
        MessageMetadata {
            actor_id: Uuid::nil(),
            correlation_id: Uuid::nil(),
            causation_id: Some(Uuid::nil()),
        }
    }

    fn sample_event() -> IntegrationEvent<TestPayload> {
        IntegrationEvent {
            event_id: Uuid::nil(),
            event_type: "user.created".to_string(),
            version: 1,
            occurred_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            metadata: sample_metadata(),
            payload: TestPayload {
                name: "alice".to_string(),
                count: 7,
            },
        }
    }

    fn sample_command() -> IntegrationCommand<TestPayload> {
        IntegrationCommand {
            command_id: Uuid::nil(),
            command_type: "notification.send".to_string(),
            version: 2,
            issued_at: DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            metadata: sample_metadata(),
            payload: TestPayload {
                name: "bob".to_string(),
                count: 3,
            },
        }
    }

    #[test]
    fn metadata_roundtrip_with_causation() {
        let meta = sample_metadata();
        let json = serde_json::to_string(&meta).unwrap();
        let back: MessageMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.actor_id, meta.actor_id);
        assert_eq!(back.correlation_id, meta.correlation_id);
        assert_eq!(back.causation_id, meta.causation_id);
    }

    #[test]
    fn metadata_skips_absent_causation() {
        let meta = MessageMetadata {
            actor_id: Uuid::nil(),
            correlation_id: Uuid::nil(),
            causation_id: None,
        };
        let json = serde_json::to_string(&meta).unwrap();
        assert!(!json.contains("causation_id"));
        let back: MessageMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.causation_id, None);
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

    #[tokio::test]
    async fn noop_publish_returns_ok() {
        let publisher = NoopIntegrationPublisher;
        publisher
            .publish(
                "identity.evt.user.created.v1",
                serde_json::json!({"k": "v"}),
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn noop_publish_if_connected_does_nothing() {
        let publisher = NoopIntegrationPublisher;
        publisher
            .publish_if_connected(
                "identity.evt.user.created.v1",
                serde_json::json!({"k": "v"}),
            )
            .await;
    }

    #[tokio::test]
    async fn ext_publish_event_through_noop() {
        let publisher = NoopIntegrationPublisher;
        let evt = sample_event();
        publisher
            .publish_event("identity.evt.user.created.v1", &evt)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ext_publish_command_through_noop() {
        let publisher = NoopIntegrationPublisher;
        let cmd = sample_command();
        publisher
            .publish_command("notifier.cmd.notification.send.v2", &cmd)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ext_fire_and_forget_through_noop() {
        let publisher = NoopIntegrationPublisher;
        let evt = sample_event();
        let cmd = sample_command();
        publisher
            .publish_event_if_connected("identity.evt.user.created.v1", &evt)
            .await;
        publisher
            .publish_command_if_connected("notifier.cmd.notification.send.v2", &cmd)
            .await;
    }

    #[tokio::test]
    async fn ext_works_through_trait_object() {
        let publisher: Arc<dyn IntegrationPublisher> = Arc::new(NoopIntegrationPublisher);
        let evt = sample_event();
        publisher
            .publish_event("identity.evt.user.created.v1", &evt)
            .await
            .unwrap();
    }

    #[test]
    fn integration_error_from_serde_json() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{ not json");
        let err: IntegrationError = bad.unwrap_err().into();
        assert!(matches!(err, IntegrationError::Serialization(_)));
    }
}
