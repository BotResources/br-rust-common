use crate::{IntegrationError, IntegrationPublisher};

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
    use crate::{IntegrationCommand, IntegrationEvent};
    use crate::{IntegrationPublisherExt, MessageMetadata};
    use br_core_kernel::{Actor, UserId};
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use uuid::Uuid;

    #[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
    struct TestPayload {
        name: String,
        count: u32,
    }

    fn sample_event() -> IntegrationEvent<TestPayload> {
        IntegrationEvent::new(
            Uuid::nil(),
            "user.created",
            1,
            DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
            MessageMetadata::new(Actor::Human(UserId::from(Uuid::nil())), Uuid::nil()),
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
            MessageMetadata::new(Actor::Human(UserId::from(Uuid::nil())), Uuid::nil()),
            TestPayload {
                name: "bob".to_string(),
                count: 3,
            },
        )
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
        publisher
            .publish_event("identity.evt.user.created.v1", &sample_event())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ext_publish_command_through_noop() {
        let publisher = NoopIntegrationPublisher;
        publisher
            .publish_command("notifier.cmd.notification.send.v2", &sample_command())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn ext_fire_and_forget_through_noop() {
        let publisher = NoopIntegrationPublisher;
        publisher
            .publish_event_if_connected("identity.evt.user.created.v1", &sample_event())
            .await;
        publisher
            .publish_command_if_connected("notifier.cmd.notification.send.v2", &sample_command())
            .await;
    }

    #[tokio::test]
    async fn ext_works_through_trait_object() {
        let publisher: Arc<dyn IntegrationPublisher> = Arc::new(NoopIntegrationPublisher);
        publisher
            .publish_event("identity.evt.user.created.v1", &sample_event())
            .await
            .unwrap();
    }
}
