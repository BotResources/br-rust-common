//! E2E tests for [`CorrelatedAwaiter`] against a real NATS JetStream broker.
//!
//! Proves: the awaiter resolves on the message carrying its `correlation_id`
//! across two filter subjects (accepted/rejected) while ignoring uncorrelated
//! and duplicate ones; it stays armed across a wait timeout (re-armable for the
//! re-publish protocol); and it fails loud when the stream is missing. See
//! `tests/common/mod.rs` for gating (`#[ignore]`, `NATS_URL`).

mod common;

use std::time::Duration;

use br_core_integration::{
    ConsumeErrorKind, CorrelatedAwaiter, IntegrationError, IntegrationEvent,
    IntegrationPublisherExt, NatsIntegrationPublisher,
};
use common::{TestPayload, create_stream, event, jetstream, teardown, unique_prefix};
use uuid::Uuid;

/// Subscribe-first, await across two subjects: the awaiter ignores an
/// uncorrelated event and a confirmation carrying a different correlation_id,
/// then resolves on the one carrying its own — and reports which subject
/// matched so the caller can decode the right payload type.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_resolves_on_correlation_across_two_subjects() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let accepted = format!("{prefix}.evt.service_scope.accepted.v1");
    let rejected = format!("{prefix}.evt.service_scope.rejected.v1");
    let js = jetstream().await;
    let _stream = create_stream(&js, &prefix).await;

    // Subscribe FIRST (before any publish), filtering both confirmation subjects.
    let mut awaiter = CorrelatedAwaiter::create(
        &js,
        format!("STREAM_{prefix}"),
        vec![accepted.clone(), rejected.clone()],
    )
    .await
    .expect("create awaiter");

    let mine = Uuid::now_v7();
    let other = Uuid::now_v7();
    let publisher = NatsIntegrationPublisher::new(js.clone());

    // Noise: an uncorrelated accepted (another replica's confirmation) and an
    // accepted carrying a different correlation_id. Then OUR rejected.
    publisher
        .publish_event(&accepted, &event("service_scope.accepted", "noise", other))
        .await
        .expect("publish noise");
    publisher
        .publish_event(&rejected, &event("service_scope.rejected", "mine", mine))
        .await
        .expect("publish mine");

    let matched = awaiter
        .await_correlation(mine, Duration::from_secs(5))
        .await
        .expect("await ok")
        .expect("a correlated match");

    // It resolved on OUR message, on the rejected subject, ignoring the noise.
    assert_eq!(matched.subject, rejected);
    assert_eq!(matched.metadata.correlation_id, mine);
    let decoded: IntegrationEvent<TestPayload> =
        serde_json::from_slice(&matched.payload).expect("decode rejected payload");
    assert_eq!(decoded.payload.label, "mine");

    teardown(&js, &prefix).await;
}

/// First match wins: a duplicate correlated confirmation (expected on the bus)
/// does not disturb the resolved result — the second wait can read it but the
/// first wait already returned the first match.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_first_match_wins_duplicates_ignored() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let accepted = format!("{prefix}.evt.service_scope.accepted.v1");
    let js = jetstream().await;
    let _stream = create_stream(&js, &prefix).await;

    let mut awaiter =
        CorrelatedAwaiter::create(&js, format!("STREAM_{prefix}"), vec![accepted.clone()])
            .await
            .expect("create awaiter");

    let mine = Uuid::now_v7();
    let publisher = NatsIntegrationPublisher::new(js.clone());
    // Duplicate confirmations, both correlated to us.
    for label in ["first", "second"] {
        publisher
            .publish_event(&accepted, &event("service_scope.accepted", label, mine))
            .await
            .expect("publish");
    }

    let first = awaiter
        .await_correlation(mine, Duration::from_secs(5))
        .await
        .expect("await ok")
        .expect("first match");
    let decoded: IntegrationEvent<TestPayload> =
        serde_json::from_slice(&first.payload).expect("decode");
    assert_eq!(
        decoded.payload.label, "first",
        "first correlated match wins"
    );

    teardown(&js, &prefix).await;
}

/// Re-armable across a timeout: a wait with no message in flight returns
/// `Ok(None)`; the awaiter stays armed, so a subsequent publish (the re-publish
/// step of the declaration handshake protocol) is caught with no gap.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_times_out_then_rearms() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let accepted = format!("{prefix}.evt.service_scope.accepted.v1");
    let js = jetstream().await;
    let _stream = create_stream(&js, &prefix).await;

    let mut awaiter =
        CorrelatedAwaiter::create(&js, format!("STREAM_{prefix}"), vec![accepted.clone()])
            .await
            .expect("create awaiter");

    let mine = Uuid::now_v7();

    // First wait: nothing published yet → times out with no match.
    let timed_out = awaiter
        .await_correlation(mine, Duration::from_millis(500))
        .await
        .expect("await ok");
    assert!(timed_out.is_none(), "no message yet → Ok(None)");

    // Re-publish (same correlation_id) and wait again on the SAME awaiter.
    let publisher = NatsIntegrationPublisher::new(js.clone());
    publisher
        .publish_event(
            &accepted,
            &event("service_scope.accepted", "after-timeout", mine),
        )
        .await
        .expect("publish");

    let matched = awaiter
        .await_correlation(mine, Duration::from_secs(5))
        .await
        .expect("await ok")
        .expect("match after re-arm");
    assert_eq!(matched.metadata.correlation_id, mine);

    teardown(&js, &prefix).await;
}

/// Regression for the silent-reap hazard: an awaiter must stay armed
/// across an idle gap *longer than the broker's default ephemeral
/// `inactive_threshold`* (nats:2-alpine ≈ 5s). We create with the default
/// `AwaiterConfig` (300s), idle ~10s with **no polling at all** (no
/// `await_correlation` call in between, so no pull requests issue), then publish
/// the correlated confirmation and assert the next wait resolves. Without the
/// explicit `inactive_threshold` the server reaps the ephemeral consumer in the
/// gap and this wait fails with `Consume { ConsumerGone }`; with it, it passes.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_stays_armed_across_long_idle_gap() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let accepted = format!("{prefix}.evt.service_scope.accepted.v1");
    let js = jetstream().await;
    let _stream = create_stream(&js, &prefix).await;

    let mut awaiter =
        CorrelatedAwaiter::create(&js, format!("STREAM_{prefix}"), vec![accepted.clone()])
            .await
            .expect("create awaiter");

    let mine = Uuid::now_v7();

    // Idle far longer than the broker's ephemeral default, WITHOUT polling: the
    // bug reaps the consumer here; the fix keeps it alive (inactive_threshold).
    tokio::time::sleep(Duration::from_secs(10)).await;

    // Now publish the correlated confirmation and await it on the SAME awaiter.
    let publisher = NatsIntegrationPublisher::new(js.clone());
    publisher
        .publish_event(
            &accepted,
            &event("service_scope.accepted", "after-idle", mine),
        )
        .await
        .expect("publish");

    let matched = awaiter
        .await_correlation(mine, Duration::from_secs(5))
        .await
        .expect("await must not fail with ConsumerGone — the awaiter stayed armed")
        .expect("a correlated match after a long idle gap");
    assert_eq!(matched.metadata.correlation_id, mine);

    teardown(&js, &prefix).await;
}

/// Fail-loud: creating an awaiter over a stream that does not exist yields
/// `Consume { kind: NoStream }` — the awaiter creates its ephemeral consumer
/// but never the stream.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_create_fails_loud_when_stream_missing() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let js = jetstream().await;

    let result = CorrelatedAwaiter::create(
        &js,
        format!("STREAM_{prefix}_absent"),
        vec![format!("{prefix}.evt.service_scope.accepted.v1")],
    )
    .await;
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
