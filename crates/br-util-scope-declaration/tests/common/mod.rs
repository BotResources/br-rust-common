#![allow(dead_code)]

use br_core_integration::{
    EventMetadata, IntegrationEvent, IntegrationPublisherExt, NatsIntegrationPublisher,
};
use br_core_kernel::{Actor, UserId};
use br_core_scope::{
    ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest, ServiceScopesAccepted,
    ServiceScopesRejected,
};
use chrono::Utc;
use futures_util::StreamExt;
use uuid::Uuid;

pub const DECLARE_SUBJECT: &str = "identity.cmd.service_scope.declare.v1";
pub const ACCEPTED_SUBJECT: &str = "identity.evt.service_scope.accepted.v1";
pub const REJECTED_SUBJECT: &str = "identity.evt.service_scope.rejected.v1";

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

pub fn unique_stream() -> String {
    format!("SCOPE_DECL_{}", Uuid::now_v7().simple())
}

pub fn notifier_declaration() -> ScopeDeclaration {
    ScopeDeclaration::new(
        ServiceManifest::new(
            ServiceKey::new("notifier").unwrap(),
            "label.notifier",
            "desc.notifier",
        ),
        vec![ScopeSpec::new(
            ScopeKey::new("notifier:read").unwrap(),
            "label.read",
            "desc.read",
            false,
        )],
    )
    .unwrap()
}

pub async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

pub async fn create_identity_stream(
    js: &async_nats::jetstream::Context,
    name: &str,
) -> async_nats::jetstream::stream::Stream {
    let _ = js.delete_stream(name).await;
    js.create_stream(async_nats::jetstream::stream::Config {
        name: name.to_string(),
        subjects: vec!["identity.>".to_string()],
        ..Default::default()
    })
    .await
    .expect("create identity stream")
}

pub async fn teardown(js: &async_nats::jetstream::Context, name: &str) {
    if let Err(e) = js.delete_stream(name).await {
        eprintln!("teardown: failed to delete stream {name}: {e}");
    }
}

#[derive(Clone, Copy)]
pub enum StubReply {
    Accept,
    Reject,
}

pub struct StubReceiver {
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for StubReceiver {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl StubReceiver {
    pub async fn spawn(
        js: &async_nats::jetstream::Context,
        stream_name: &str,
        reply: StubReply,
        duplicates: usize,
    ) -> Self {
        let stream = js.get_stream(stream_name).await.expect("stub: get stream");
        let consumer = stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: None,
                deliver_policy: async_nats::jetstream::consumer::DeliverPolicy::New,
                ack_policy: async_nats::jetstream::consumer::AckPolicy::None,
                filter_subject: DECLARE_SUBJECT.to_string(),
                ..Default::default()
            })
            .await
            .expect("stub: create consumer");
        let mut messages = consumer.messages().await.expect("stub: messages");

        let publisher = NatsIntegrationPublisher::new(js.clone());
        let duplicates = duplicates.max(1);

        let handle = tokio::spawn(async move {
            while let Some(Ok(msg)) = messages.next().await {
                let probe: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let correlation_id = probe
                    .get("metadata")
                    .and_then(|m| m.get("correlation_id"))
                    .and_then(|c| c.as_str())
                    .and_then(|s| s.parse::<Uuid>().ok());
                let Some(correlation_id) = correlation_id else {
                    continue;
                };

                for _ in 0..duplicates {
                    match reply {
                        StubReply::Accept => {
                            let event = accepted_event(correlation_id);
                            let _ = publisher.publish_event(ACCEPTED_SUBJECT, &event).await;
                        }
                        StubReply::Reject => {
                            let event = rejected_event(correlation_id);
                            let _ = publisher.publish_event(REJECTED_SUBJECT, &event).await;
                        }
                    }
                }
            }
        });

        Self { handle }
    }
}

pub async fn spawn_delayed_accept_stub(
    js: &async_nats::jetstream::Context,
    stream_name: &str,
    ignore_first: usize,
) -> StubReceiver {
    let stream = js.get_stream(stream_name).await.expect("stub: get stream");
    let consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: None,
            deliver_policy: async_nats::jetstream::consumer::DeliverPolicy::New,
            ack_policy: async_nats::jetstream::consumer::AckPolicy::None,
            filter_subject: DECLARE_SUBJECT.to_string(),
            ..Default::default()
        })
        .await
        .expect("stub: create consumer");
    let mut messages = consumer.messages().await.expect("stub: messages");
    let publisher = NatsIntegrationPublisher::new(js.clone());

    let handle = tokio::spawn(async move {
        let mut seen = 0usize;
        while let Some(Ok(msg)) = messages.next().await {
            seen += 1;
            if seen <= ignore_first {
                continue;
            }
            let probe: serde_json::Value = match serde_json::from_slice(&msg.payload) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let correlation_id = probe
                .get("metadata")
                .and_then(|m| m.get("correlation_id"))
                .and_then(|c| c.as_str())
                .and_then(|s| s.parse::<Uuid>().ok());
            if let Some(correlation_id) = correlation_id {
                let event = accepted_event(correlation_id);
                let _ = publisher.publish_event(ACCEPTED_SUBJECT, &event).await;
            }
        }
    });

    StubReceiver { handle }
}

pub async fn declare_message_count(js: &async_nats::jetstream::Context, stream_name: &str) -> u64 {
    let mut stream = js.get_stream(stream_name).await.expect("get stream");
    let info = stream.info().await.expect("stream info");
    info.state.messages
}

fn metadata(correlation_id: Uuid) -> EventMetadata {
    EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id)
}

fn accepted_event(correlation_id: Uuid) -> IntegrationEvent<ServiceScopesAccepted> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "service_scope.accepted",
        1,
        Utc::now(),
        metadata(correlation_id),
        ServiceScopesAccepted::new(ServiceKey::new("notifier").unwrap()),
    )
}

fn rejected_event(correlation_id: Uuid) -> IntegrationEvent<ServiceScopesRejected> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "service_scope.rejected",
        1,
        Utc::now(),
        metadata(correlation_id),
        ServiceScopesRejected::new(
            ServiceKey::new("notifier").unwrap(),
            br_core_scope::ScopeDeclarationError::ScopeOwnedByAnotherService {
                key: "notifier:read".to_string(),
                owner: "billing".to_string(),
            },
        ),
    )
}
