#![allow(dead_code)]

use br_core_integration::{EventMetadata, IntegrationEvent};
use br_core_kernel::{Actor, UserId};
use br_core_scope::{
    ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest, ServiceScopesAccepted,
    ServiceScopesRejected,
};
use br_scope_declaration_contract::{accepted_event_coords, rejected_event_coords};
use br_util_nats_fabric::{Fabric, INTEGRATION_CMD, INTEGRATION_EVT};
use chrono::Utc;
use futures_util::StreamExt;
use uuid::Uuid;

pub const DECLARE_SUBJECT: &str = "integration.cmd.identity.service_scope.declare.v1";

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

static FABRIC_STREAM_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();

pub async fn serialize_fabric_streams() -> tokio::sync::MutexGuard<'static, ()> {
    FABRIC_STREAM_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await
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

pub async fn fabric() -> Fabric {
    Fabric::new(jetstream().await)
}

pub async fn create_fabric_streams(js: &async_nats::jetstream::Context) {
    recreate(js, INTEGRATION_CMD, "integration.cmd.>").await;
    recreate(js, INTEGRATION_EVT, "integration.evt.>").await;
}

async fn recreate(js: &async_nats::jetstream::Context, name: &str, bind: &str) {
    let _ = js.delete_stream(name).await;
    js.create_stream(async_nats::jetstream::stream::Config {
        name: name.to_string(),
        subjects: vec![bind.to_string()],
        ..Default::default()
    })
    .await
    .expect("create fixed fabric stream");
}

pub async fn teardown(js: &async_nats::jetstream::Context) {
    let _ = js.delete_stream(INTEGRATION_CMD).await;
    let _ = js.delete_stream(INTEGRATION_EVT).await;
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
        reply: StubReply,
        duplicates: usize,
    ) -> Self {
        let mut messages = declare_consumer(js).await;
        let fabric = Fabric::new(js.clone());
        let duplicates = duplicates.max(1);

        let handle = tokio::spawn(async move {
            while let Some(Ok(msg)) = messages.next().await {
                let Some(correlation_id) = correlation_of(&msg.payload) else {
                    continue;
                };
                for _ in 0..duplicates {
                    publish_reply(&fabric, reply, correlation_id).await;
                }
            }
        });

        Self { handle }
    }
}

pub async fn spawn_delayed_accept_stub(
    js: &async_nats::jetstream::Context,
    ignore_first: usize,
) -> StubReceiver {
    let mut messages = declare_consumer(js).await;
    let fabric = Fabric::new(js.clone());

    let handle = tokio::spawn(async move {
        let mut seen = 0usize;
        while let Some(Ok(msg)) = messages.next().await {
            seen += 1;
            if seen <= ignore_first {
                continue;
            }
            if let Some(correlation_id) = correlation_of(&msg.payload) {
                publish_reply(&fabric, StubReply::Accept, correlation_id).await;
            }
        }
    });

    StubReceiver { handle }
}

async fn declare_consumer(
    js: &async_nats::jetstream::Context,
) -> async_nats::jetstream::consumer::pull::Stream {
    let stream = js
        .get_stream(INTEGRATION_CMD)
        .await
        .expect("stub: get stream");
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
    consumer.messages().await.expect("stub: messages")
}

async fn publish_reply(fabric: &Fabric, reply: StubReply, correlation_id: Uuid) {
    match reply {
        StubReply::Accept => {
            let coords = accepted_event_coords().unwrap();
            let _ = fabric
                .publish_event(&coords, &accepted_event(correlation_id))
                .await;
        }
        StubReply::Reject => {
            let coords = rejected_event_coords().unwrap();
            let _ = fabric
                .publish_event(&coords, &rejected_event(correlation_id))
                .await;
        }
    }
}

fn correlation_of(payload: &[u8]) -> Option<Uuid> {
    let probe: serde_json::Value = serde_json::from_slice(payload).ok()?;
    probe
        .get("metadata")
        .and_then(|m| m.get("correlation_id"))
        .and_then(|c| c.as_str())
        .and_then(|s| s.parse::<Uuid>().ok())
}

pub async fn declare_message_count(js: &async_nats::jetstream::Context) -> u64 {
    let mut stream = js.get_stream(INTEGRATION_CMD).await.expect("get stream");
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
