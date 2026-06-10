//! E2E tests for `NatsIntegrationPublisher` against a real NATS JetStream
//! broker. Same risk class as the `br-util-postgres` e2e suite: unit tests
//! can't exercise the wire format or the JetStream ack contract, so the
//! kinds of regressions that bite production (subject pattern mismatch,
//! payload corruption, swallowed publish errors) only show up here.
//!
//! Run gating:
//!   - `#[ignore]` by default, opted into via `cargo test -- --ignored`.
//!   - `NATS_URL` env var must point to a JetStream-enabled NATS instance.
//!     CI runs `nats:2-alpine -js` in a sidecar container; locally use
//!     `docker run -d --rm -p 4222:4222 -p 8222:8222 nats:2-alpine -js -m 8222`
//!     and set `NATS_URL=nats://localhost:4222`.
//!
//! Each test creates its own JetStream stream with a unique subject prefix
//! to avoid cross-test interference even under parallel execution.

use br_core_integration::{
    IntegrationCommand, IntegrationError, IntegrationEvent, IntegrationPublisher,
    IntegrationPublisherExt, MessageMetadata, NatsIntegrationPublisher, PublishErrorKind,
};
use br_core_kernel::{Actor, UserId};
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Minimal payload exercised by the roundtrip tests. Keeps the assertion
/// surface small while covering the typical event/command shape (string +
/// numeric field, both round-tripped through serde_json).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct TestPayload {
    name: String,
    count: u32,
}

fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

/// Unique per-test stream/subject prefix so parallel tests don't trip
/// each other (e.g., one test's stream capturing another's messages).
///
/// Uses the FULL uuid: truncating a v7 to its first 16 hex chars keeps mostly
/// the millisecond timestamp (~12 random bits left), and two tests starting
/// in the same millisecond collided in practice — one stream captured the
/// other's messages.
fn unique_prefix() -> String {
    let suffix = Uuid::now_v7().simple().to_string();
    format!("br_test_{suffix}")
}

fn sample_metadata() -> MessageMetadata {
    MessageMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), Uuid::now_v7())
}

/// Connect, create an ephemeral stream that captures `subject_pattern`,
/// and return the publisher (driving the same client) plus the stream so
/// the test can pull messages back out.
async fn setup(
    subject_pattern: String,
    stream_name: String,
) -> (
    NatsIntegrationPublisher,
    async_nats::jetstream::stream::Stream,
) {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    let js = async_nats::jetstream::new(client);

    // Always start from a clean slate — a previous failed run could have
    // left the same stream around.
    let _ = js.delete_stream(&stream_name).await;
    let stream = js
        .create_stream(async_nats::jetstream::stream::Config {
            name: stream_name,
            subjects: vec![subject_pattern],
            ..Default::default()
        })
        .await
        .expect("create stream");

    (NatsIntegrationPublisher::new(js), stream)
}

async fn teardown(stream: async_nats::jetstream::stream::Stream) {
    // Best-effort: deleting the stream removes all stored messages so the next
    // test (or rerun) starts clean. A failure here doesn't fail the test (the
    // assertions already ran), but it must not be fully silent — a leaked
    // stream can capture a later test's messages, so surface it loudly enough
    // to diagnose a flaky run.
    let name = stream.cached_info().config.name.clone();
    let url = nats_url().unwrap_or_default();
    match async_nats::connect(&url).await {
        Ok(client) => {
            let js = async_nats::jetstream::new(client);
            if let Err(e) = js.delete_stream(&name).await {
                eprintln!("teardown: failed to delete stream {name}: {e}");
            }
        }
        Err(e) => eprintln!("teardown: failed to reconnect to delete stream {name}: {e}"),
    }
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn publish_event_roundtrips_through_jetstream() {
    let Some(_) = nats_url() else { return };
    let prefix = unique_prefix();
    let subject = format!("{prefix}.evt.user.created.v1");
    let (publisher, stream) = setup(format!("{prefix}.>"), format!("STREAM_{prefix}")).await;

    let event = IntegrationEvent::new(
        Uuid::now_v7(),
        "user.created",
        1,
        DateTime::<Utc>::from_timestamp(1_700_000_000, 0).unwrap(),
        sample_metadata(),
        TestPayload {
            name: "alice".to_string(),
            count: 7,
        },
    );

    publisher
        .publish_event(&subject, &event)
        .await
        .expect("publish_event");

    // Pull the message back and deserialize to verify byte-exact roundtrip.
    let consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("test_consumer".to_string()),
            ..Default::default()
        })
        .await
        .expect("create consumer");
    let mut batch = consumer
        .fetch()
        .max_messages(1)
        .messages()
        .await
        .expect("fetch");
    let msg = batch
        .next()
        .await
        .expect("a message was delivered")
        .expect("delivery ok");
    assert_eq!(msg.subject.as_str(), subject);
    let decoded: IntegrationEvent<TestPayload> =
        serde_json::from_slice(&msg.payload).expect("payload deserializes");
    assert_eq!(decoded.event_id, event.event_id);
    assert_eq!(decoded.event_type, "user.created");
    assert_eq!(decoded.payload, event.payload);
    msg.ack().await.expect("ack");

    teardown(stream).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn publish_command_roundtrips_through_jetstream() {
    let Some(_) = nats_url() else { return };
    let prefix = unique_prefix();
    let subject = format!("{prefix}.cmd.notification.send.v1");
    let (publisher, stream) = setup(format!("{prefix}.>"), format!("STREAM_{prefix}")).await;

    let command = IntegrationCommand::new(
        Uuid::now_v7(),
        "notification.send",
        1,
        DateTime::<Utc>::from_timestamp(1_700_000_001, 0).unwrap(),
        sample_metadata(),
        TestPayload {
            name: "bob".to_string(),
            count: 42,
        },
    );

    publisher
        .publish_command(&subject, &command)
        .await
        .expect("publish_command");

    let consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("test_consumer".to_string()),
            ..Default::default()
        })
        .await
        .expect("create consumer");
    let mut batch = consumer
        .fetch()
        .max_messages(1)
        .messages()
        .await
        .expect("fetch");
    let msg = batch
        .next()
        .await
        .expect("a message was delivered")
        .expect("delivery ok");
    let decoded: IntegrationCommand<TestPayload> =
        serde_json::from_slice(&msg.payload).expect("payload deserializes");
    assert_eq!(decoded.command_id, command.command_id);
    assert_eq!(decoded.command_type, "notification.send");
    assert_eq!(decoded.payload, command.payload);
    msg.ack().await.expect("ack");

    teardown(stream).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn publish_returns_err_when_no_stream_matches() {
    // The publisher awaits the JetStream ack. If no stream is configured
    // for the subject, JetStream replies with "no responders" / "no stream
    // matched" — `publish()` must surface that as IntegrationError::Publish
    // rather than silently succeeding.
    let Some(url) = nats_url() else { return };
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    let js = async_nats::jetstream::new(client);
    let publisher = NatsIntegrationPublisher::new(js);

    let prefix = unique_prefix();
    let subject = format!("{prefix}.orphan.no_stream.v1");
    let result = publisher
        .publish(&subject, serde_json::json!({"hello": "world"}))
        .await;

    assert!(
        matches!(
            result,
            Err(IntegrationError::Publish {
                kind: PublishErrorKind::NoStream,
                ..
            })
        ),
        "expected Publish{{ kind: NoStream }} for unmatched subject, got {result:?}"
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn publish_if_connected_swallows_no_stream_error() {
    // The fire-and-forget variant must never propagate — it should log
    // and return. Verified by calling against an unmatched subject and
    // asserting only that the call completes without panic.
    let Some(url) = nats_url() else { return };
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    let js = async_nats::jetstream::new(client);
    let publisher = NatsIntegrationPublisher::new(js);

    let prefix = unique_prefix();
    let subject = format!("{prefix}.orphan.no_stream.v1");
    publisher
        .publish_if_connected(&subject, serde_json::json!({"hello": "world"}))
        .await;
}
