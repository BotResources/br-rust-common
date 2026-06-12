mod common;

use std::time::Duration;

use br_core_integration::{
    ConsumeErrorKind, CorrelatedAwaiter, IntegrationError, IntegrationEvent,
    IntegrationPublisherExt, NatsIntegrationPublisher,
};
use common::{TestPayload, create_stream, event, jetstream, teardown, unique_prefix};
use uuid::Uuid;

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_resolves_on_correlation_across_two_subjects() {
    let Some(_) = common::nats_url() else { return };
    let prefix = unique_prefix();
    let accepted = format!("{prefix}.evt.service_scope.accepted.v1");
    let rejected = format!("{prefix}.evt.service_scope.rejected.v1");
    let js = jetstream().await;
    let _stream = create_stream(&js, &prefix).await;

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

    assert_eq!(matched.subject, rejected);
    assert_eq!(matched.metadata.correlation_id, mine);
    let decoded: IntegrationEvent<TestPayload> =
        serde_json::from_slice(&matched.payload).expect("decode rejected payload");
    assert_eq!(decoded.payload.label, "mine");

    teardown(&js, &prefix).await;
}

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

    let timed_out = awaiter
        .await_correlation(mine, Duration::from_millis(500))
        .await
        .expect("await ok");
    assert!(timed_out.is_none(), "no message yet → Ok(None)");

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

    tokio::time::sleep(Duration::from_secs(10)).await;

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
