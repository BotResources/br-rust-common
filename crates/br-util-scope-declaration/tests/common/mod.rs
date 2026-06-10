//! Shared helpers for the handshake e2e suite, including the **stub receiver**
//! that stands in for Identity (subscribe to the declare command subject, reply
//! accepted/rejected echoing the `correlation_id`).
//!
//! Gating mirrors `br-core-integration`'s e2e: `#[ignore]` by default, opted
//! into via `cargo test -- --ignored`, requiring `NATS_URL` to point at a
//! JetStream-enabled broker. The suite runs `--test-threads=1`: every test
//! captures the fixed `identity.>` subjects (the helper builds the real subject
//! names), so the streams overlap and must exist one at a time — each test
//! creates a uniquely-named stream and tears it down at the end.

#![allow(dead_code)]

use br_core_integration::{
    IntegrationEvent, IntegrationPublisherExt, MessageMetadata, NatsIntegrationPublisher,
};
use br_core_kernel::{Actor, UserId};
use br_core_scope::{
    ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest, ServiceScopesAccepted,
    ServiceScopesRejected,
};
use chrono::Utc;
use futures_util::StreamExt;
use uuid::Uuid;

/// The two confirmation subjects the helper awaits, and the command subject it
/// publishes to. Kept here verbatim so the e2e binds the *exact* wire contract
/// the helper drives (a drift between them would be the bug we want to catch).
pub const DECLARE_SUBJECT: &str = "identity.cmd.service_scope.declare.v1";
pub const ACCEPTED_SUBJECT: &str = "identity.evt.service_scope.accepted.v1";
pub const REJECTED_SUBJECT: &str = "identity.evt.service_scope.rejected.v1";

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

/// Unique per-test stream name. The subjects are fixed (`identity.>`), so only
/// the name varies; with `--test-threads=1` + teardown, one stream exists at a
/// time.
pub fn unique_stream() -> String {
    format!("SCOPE_DECL_{}", Uuid::now_v7().simple())
}

/// A valid `notifier` declaration with one read scope.
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

/// Connect and return a JetStream context.
pub async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

/// Create a stream `name` capturing both the command and the confirmation
/// subjects (`identity.>`). Starts clean (deletes any leftover).
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

/// Delete the stream (best-effort, loud).
pub async fn teardown(js: &async_nats::jetstream::Context, name: &str) {
    if let Err(e) = js.delete_stream(name).await {
        eprintln!("teardown: failed to delete stream {name}: {e}");
    }
}

/// How the stub Identity should reply to a declaration.
#[derive(Clone, Copy)]
pub enum StubReply {
    Accept,
    Reject,
}

/// The stub receiver: an ephemeral pull consumer over the declare subject that,
/// on each received command, replies on the matching confirmation subject
/// **echoing the command's `correlation_id`**. It replies `times` times to the
/// FIRST command it sees (to exercise duplicate confirmations), then keeps
/// echoing one reply per subsequent command (to exercise the re-publish path).
///
/// Spawned as a background task; returns a handle that is aborted on drop.
pub struct StubReceiver {
    handle: tokio::task::JoinHandle<()>,
}

impl Drop for StubReceiver {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

impl StubReceiver {
    /// Spawn the stub. `reply` selects accept/reject; `duplicates` is how many
    /// identical confirmations to emit per received command (≥1; >1 exercises
    /// the first-match-wins duplicate tolerance).
    pub async fn spawn(
        js: &async_nats::jetstream::Context,
        stream_name: &str,
        reply: StubReply,
        duplicates: usize,
    ) -> Self {
        let stream = js.get_stream(stream_name).await.expect("stub: get stream");
        // Ephemeral pull consumer filtered to the declare subject; New deliver
        // policy so it only sees commands published after it is armed.
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
                // Read the correlation_id off the command envelope (probe only).
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

/// A delayed stub that ignores the first `ignore_first` commands (forcing the
/// helper to time out and re-publish), then replies `Accept` to the next one —
/// echoing its `correlation_id`. Exercises timeout → re-publish → accepted.
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
                continue; // swallow → helper times out and re-publishes
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

/// Count of messages currently captured on the declare subject — used to assert
/// the disabled mode published **nothing**.
pub async fn declare_message_count(js: &async_nats::jetstream::Context, stream_name: &str) -> u64 {
    let mut stream = js.get_stream(stream_name).await.expect("get stream");
    let info = stream.info().await.expect("stream info");
    // The stream captures only `identity.>`; in the disabled-mode test nothing
    // else is published, so the message count is the declare-command count.
    info.state.messages
}

fn metadata(correlation_id: Uuid) -> MessageMetadata {
    MessageMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id)
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
