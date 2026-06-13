mod common;

use std::time::Duration;

use br_util_axum_readiness::ReadinessHandle;
use br_util_scope_declaration::{ScopeDeclarationConfig, ScopeDeclarationOutcome, declare_scopes};
use common::{
    StubReceiver, StubReply, create_identity_stream, declare_message_count, jetstream,
    notifier_declaration, spawn_delayed_accept_stub, teardown, unique_stream,
};

fn fast_config(stream_name: &str) -> ScopeDeclarationConfig {
    let mut config = ScopeDeclarationConfig::enabled(stream_name);
    config.wait_timeout = Duration::from_millis(500);
    config
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn accepted_sets_readiness_up() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let stream = unique_stream();
    let _s = create_identity_stream(&js, &stream).await;
    let _stub = StubReceiver::spawn(&js, &stream, StubReply::Accept, 1).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &js,
            notifier_declaration(),
            readiness.clone(),
            fast_config(&stream),
        ),
    )
    .await
    .expect("handshake completed within the timeout")
    .expect("handshake ok");

    assert!(matches!(outcome, ScopeDeclarationOutcome::Accepted));
    assert!(readiness.is_ready(), "accepted → readiness UP");

    teardown(&js, &stream).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn rejected_sets_readiness_down_with_reason() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let stream = unique_stream();
    let _s = create_identity_stream(&js, &stream).await;
    let _stub = StubReceiver::spawn(&js, &stream, StubReply::Reject, 1).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &js,
            notifier_declaration(),
            readiness.clone(),
            fast_config(&stream),
        ),
    )
    .await
    .expect("handshake completed within the timeout")
    .expect("handshake ok");

    match outcome {
        ScopeDeclarationOutcome::Rejected(reason) => {
            assert_eq!(reason.reason.to_string(), "scope_owned_by_another_service");
            assert_eq!(reason.service.as_str(), "notifier");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(!readiness.is_ready(), "rejected → readiness DOWN");

    teardown(&js, &stream).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn timeout_then_republish_then_accepted() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let stream = unique_stream();
    let _s = create_identity_stream(&js, &stream).await;
    let _stub = spawn_delayed_accept_stub(&js, &stream, 1).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &js,
            notifier_declaration(),
            readiness.clone(),
            fast_config(&stream),
        ),
    )
    .await
    .expect("handshake completed after a re-publish")
    .expect("handshake ok");

    assert!(matches!(outcome, ScopeDeclarationOutcome::Accepted));
    assert!(
        readiness.is_ready(),
        "accepted after re-publish → readiness UP"
    );

    teardown(&js, &stream).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn duplicate_confirmations_first_match_wins() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let stream = unique_stream();
    let _s = create_identity_stream(&js, &stream).await;
    let _stub = StubReceiver::spawn(&js, &stream, StubReply::Accept, 2).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &js,
            notifier_declaration(),
            readiness.clone(),
            fast_config(&stream),
        ),
    )
    .await
    .expect("handshake completed within the timeout")
    .expect("handshake ok");

    assert!(matches!(outcome, ScopeDeclarationOutcome::Accepted));
    assert!(readiness.is_ready(), "duplicate accepted → still just UP");

    teardown(&js, &stream).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn disabled_mode_publishes_nothing_and_sets_ready() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let stream = unique_stream();
    let _s = create_identity_stream(&js, &stream).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = declare_scopes(
        &js,
        notifier_declaration(),
        readiness.clone(),
        ScopeDeclarationConfig::disabled(&stream),
    )
    .await
    .expect("disabled handshake ok");

    assert!(matches!(outcome, ScopeDeclarationOutcome::Disabled));
    assert!(readiness.is_ready(), "disabled → readiness UP");

    tokio::time::sleep(Duration::from_millis(300)).await;
    let count = declare_message_count(&js, &stream).await;
    assert_eq!(count, 0, "disabled mode must publish NO declare command");

    teardown(&js, &stream).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn missing_stream_fails_loud() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let absent = format!("{}_absent", unique_stream());

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        declare_scopes(
            &js,
            notifier_declaration(),
            readiness.clone(),
            fast_config(&absent),
        ),
    )
    .await
    .expect("returned without hanging");

    assert!(result.is_err(), "missing stream → fail loud (Err)");
    assert!(!readiness.is_ready(), "fail-loud leaves readiness DOWN");
}
