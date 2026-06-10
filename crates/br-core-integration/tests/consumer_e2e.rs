//! E2E tests for [`DurableConsumer`] against a real NATS JetStream broker.
//!
//! Proves what unit tests cannot: durable delivery + typed decode over the
//! wire, redelivery on nak, term (no infinite redelivery) for a poison message,
//! and the fail-loud bind contract when a declared stream or consumer is
//! missing. See `tests/common/mod.rs` for gating (`#[ignore]`, `NATS_URL`).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use br_core_integration::{
    ConsumeErrorKind, Delivery, DurableConsumer, IntegrationCommand, IntegrationError,
    IntegrationPublisher, IntegrationPublisherExt, MessageOutcome, NatsIntegrationPublisher,
};
use common::{
    TestPayload, command, create_durable, create_stream, jetstream, teardown, unique_prefix,
};
use tokio::sync::mpsc;

/// Durable delivery: a published command is decoded into the typed envelope and
/// delivered to the handler, which acks it.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn durable_consumer_delivers_and_acks_command() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let subject = format!("{prefix}.cmd.service_scope.declare.v1");
    let js = jetstream().await;
    let stream = create_stream(&js, &prefix).await;
    create_durable(&stream, "declare_worker", &subject).await;

    let publisher = NatsIntegrationPublisher::new(js.clone());
    publisher
        .publish_command(&subject, &command("hello", uuid::Uuid::now_v7()))
        .await
        .expect("publish");

    let (tx, mut rx) = mpsc::channel::<String>(4);
    let consumer = DurableConsumer::bind(&js, format!("STREAM_{prefix}"), "declare_worker")
        .await
        .expect("bind");
    let task = tokio::spawn(async move {
        consumer
            .run_commands(
                |d: Delivery<IntegrationCommand<TestPayload>>| {
                    let tx = tx.clone();
                    async move {
                        tx.send(d.envelope.payload.label.clone()).await.ok();
                        MessageOutcome::Ack
                    }
                },
                |err| panic!("unexpected poison: {err}"),
            )
            .await
            .ok();
    });

    let got = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("handler ran within 5s")
        .expect("delivery");
    assert_eq!(got, "hello");

    task.abort();
    teardown(&js, &prefix).await;
}

/// Redelivery on nak: the first delivery naks, the redelivery acks. The handler
/// sees the same message twice — proving nak triggers redelivery.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn durable_consumer_redelivers_on_nak() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let subject = format!("{prefix}.cmd.service_scope.declare.v1");
    let js = jetstream().await;
    let stream = create_stream(&js, &prefix).await;
    create_durable(&stream, "nak_worker", &subject).await;

    let publisher = NatsIntegrationPublisher::new(js.clone());
    publisher
        .publish_command(&subject, &command("retry-me", uuid::Uuid::now_v7()))
        .await
        .expect("publish");

    let deliveries = Arc::new(AtomicUsize::new(0));
    let (done_tx, mut done_rx) = mpsc::channel::<()>(2);
    let consumer = DurableConsumer::bind(&js, format!("STREAM_{prefix}"), "nak_worker")
        .await
        .expect("bind");
    let deliveries_for_task = deliveries.clone();
    let task = tokio::spawn(async move {
        consumer
            .run_commands(
                move |_d: Delivery<IntegrationCommand<TestPayload>>| {
                    let n = deliveries_for_task.fetch_add(1, Ordering::SeqCst);
                    let done_tx = done_tx.clone();
                    async move {
                        done_tx.send(()).await.ok();
                        if n == 0 {
                            // First delivery: ask for an immediate redelivery.
                            MessageOutcome::Nak(Some(Duration::from_millis(50)))
                        } else {
                            MessageOutcome::Ack
                        }
                    }
                },
                |err| panic!("unexpected poison: {err}"),
            )
            .await
            .ok();
    });

    // Two deliveries expected: original + redelivery.
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(5), done_rx.recv())
            .await
            .expect("a (re)delivery within 5s")
            .expect("delivery");
    }
    assert!(
        deliveries.load(Ordering::SeqCst) >= 2,
        "nak triggered redelivery"
    );

    task.abort();
    teardown(&js, &prefix).await;
}

/// Poison message: a payload that does not deserialize into the typed envelope
/// is termed (not redelivered forever) and surfaced via `on_poison`. Proven by
/// publishing garbage, asserting `on_poison` fires once, and confirming no
/// further redelivery arrives.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn durable_consumer_terms_poison_message() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let subject = format!("{prefix}.cmd.service_scope.declare.v1");
    let js = jetstream().await;
    let stream = create_stream(&js, &prefix).await;
    create_durable(&stream, "poison_worker", &subject).await;

    // Publish non-envelope bytes directly so they capture into the stream.
    let publisher = NatsIntegrationPublisher::new(js.clone());
    publisher
        .publish(&subject, serde_json::json!({ "garbage": true }))
        .await
        .expect("publish garbage");

    let (poison_tx, mut poison_rx) = mpsc::channel::<String>(4);
    let consumer = DurableConsumer::bind(&js, format!("STREAM_{prefix}"), "poison_worker")
        .await
        .expect("bind");
    let task = tokio::spawn(async move {
        consumer
            .run_commands(
                |_d: Delivery<IntegrationCommand<TestPayload>>| async {
                    panic!("poison must never reach the handler")
                },
                move |err: IntegrationError| {
                    let poison_tx = poison_tx.clone();
                    let subject = match &err {
                        IntegrationError::Decode { subject, .. } => subject.clone(),
                        other => panic!("expected Decode, got {other:?}"),
                    };
                    // `on_poison` is sync; hand the subject to the test thread.
                    poison_tx.try_send(subject).ok();
                },
            )
            .await
            .ok();
    });

    let poisoned = tokio::time::timeout(Duration::from_secs(5), poison_rx.recv())
        .await
        .expect("poison surfaced within 5s")
        .expect("poison subject");
    assert_eq!(poisoned, subject);

    // No redelivery: ack-wait is 2s; if it had not been termed, a second poison
    // would surface. Assert none arrives within a window past ack-wait.
    let redelivered = tokio::time::timeout(Duration::from_secs(4), poison_rx.recv()).await;
    assert!(
        redelivered.is_err(),
        "termed poison message must not be redelivered, got {redelivered:?}"
    );

    task.abort();
    teardown(&js, &prefix).await;
}

/// Fail-loud: binding to a stream that does not exist yields
/// `Consume { kind: NoStream }` — the lib never creates it.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn bind_fails_loud_when_stream_missing() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let js = jetstream().await;

    let result = DurableConsumer::bind(&js, format!("STREAM_{prefix}_absent"), "w").await;
    assert!(
        matches!(
            result.as_ref().map(|_| ()),
            Err(IntegrationError::Consume {
                kind: ConsumeErrorKind::NoStream,
                ..
            })
        ),
        "expected Consume{{ NoStream }}, got {:?}",
        result.map(|_| ())
    );
}

/// Fail-loud: the stream exists but the named durable consumer does not →
/// `Consume { kind: NoConsumer }` — the durable wrapper never creates it.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn bind_fails_loud_when_consumer_missing() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let js = jetstream().await;
    let _stream = create_stream(&js, &prefix).await;

    let result = DurableConsumer::bind(&js, format!("STREAM_{prefix}"), "never_declared").await;
    let matched = matches!(
        result.as_ref().map(|_| ()),
        Err(IntegrationError::Consume {
            kind: ConsumeErrorKind::NoConsumer,
            ..
        })
    );
    teardown(&js, &prefix).await;
    assert!(
        matched,
        "expected Consume{{ NoConsumer }} for an undeclared consumer"
    );
}

/// Zero-CPU idle: a bound consumer with no messages parks on
/// `consumer.messages()` (never a `fetch()` busy-loop). Proven by sampling this
/// process's cumulative CPU time across a multi-second idle window and asserting
/// the consumer burns negligible CPU while parked — a spin loop would show up as
/// seconds of CPU here.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn durable_consumer_idles_at_zero_cpu() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let subject = format!("{prefix}.cmd.service_scope.declare.v1");
    let js = jetstream().await;
    let stream = create_stream(&js, &prefix).await;
    create_durable(&stream, "idle_worker", &subject).await;

    let consumer = DurableConsumer::bind(&js, format!("STREAM_{prefix}"), "idle_worker")
        .await
        .expect("bind");
    let task = tokio::spawn(async move {
        // Park: no messages are ever published, so the handler never runs.
        consumer
            .run_commands(
                |_d: Delivery<IntegrationCommand<TestPayload>>| async { MessageOutcome::Ack },
                |err| panic!("unexpected poison: {err}"),
            )
            .await
            .ok();
    });

    // Let the consumer settle, then measure CPU time over a fixed idle window.
    // If `ps` is unavailable we cannot sample CPU — skip the assertion cleanly
    // rather than fail on the environment (the parking behaviour is unchanged).
    tokio::time::sleep(Duration::from_millis(500)).await;
    let idle_window = Duration::from_secs(3);
    let before = common::process_cpu_seconds();
    tokio::time::sleep(idle_window).await;
    let after = common::process_cpu_seconds();

    task.abort();
    teardown(&js, &prefix).await;

    let (Some(before), Some(after)) = (before, after) else {
        eprintln!("durable_consumer_idles_at_zero_cpu: `ps` unavailable, skipping CPU assertion");
        return;
    };
    let consumed = after - before;

    // A parked consumer should consume well under a second of CPU over 3s of
    // wall time. A `fetch()` spin loop would burn most of the window.
    assert!(
        consumed < 0.5,
        "parked consumer burned {consumed:.3}s CPU over {}s idle — expected a parked, \
         zero-CPU consumer, not a polling loop",
        idle_window.as_secs()
    );
}
