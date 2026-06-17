mod common;

use std::time::Duration;

use br_util_axum_readiness::ReadinessHandle;
use br_util_nats_fabric::{INTEGRATION_CMD, INTEGRATION_EVT};
use br_util_scope_declaration::{ScopeDeclarationConfig, ScopeDeclarationOutcome, declare_scopes};
use common::{
    StubReceiver, StubReply, create_fabric_streams, declare_message_count, fabric, jetstream,
    notifier_declaration, serialize_fabric_streams, spawn_delayed_accept_stub, teardown,
};

fn fast_config() -> ScopeDeclarationConfig {
    let mut config = ScopeDeclarationConfig::enabled();
    config.wait_timeout = Duration::from_millis(500);
    config
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn accepted_sets_readiness_up() {
    let Some(_) = common::nats_url() else { return };
    let _guard = serialize_fabric_streams().await;
    let js = jetstream().await;
    create_fabric_streams(&js).await;
    let _stub = StubReceiver::spawn(&js, StubReply::Accept, 1).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &fabric().await,
            notifier_declaration(),
            readiness.clone(),
            fast_config(),
        ),
    )
    .await
    .expect("handshake completed within the timeout")
    .expect("handshake ok");

    assert!(matches!(outcome, ScopeDeclarationOutcome::Accepted));
    assert!(readiness.is_ready(), "accepted → readiness UP");

    teardown(&js).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn rejected_sets_readiness_down_with_reason() {
    let Some(_) = common::nats_url() else { return };
    let _guard = serialize_fabric_streams().await;
    let js = jetstream().await;
    create_fabric_streams(&js).await;
    let _stub = StubReceiver::spawn(&js, StubReply::Reject, 1).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &fabric().await,
            notifier_declaration(),
            readiness.clone(),
            fast_config(),
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

    teardown(&js).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn timeout_then_republish_then_accepted() {
    let Some(_) = common::nats_url() else { return };
    let _guard = serialize_fabric_streams().await;
    let js = jetstream().await;
    create_fabric_streams(&js).await;
    let _stub = spawn_delayed_accept_stub(&js, 1).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &fabric().await,
            notifier_declaration(),
            readiness.clone(),
            fast_config(),
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

    teardown(&js).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn duplicate_confirmations_first_match_wins() {
    let Some(_) = common::nats_url() else { return };
    let _guard = serialize_fabric_streams().await;
    let js = jetstream().await;
    create_fabric_streams(&js).await;
    let _stub = StubReceiver::spawn(&js, StubReply::Accept, 2).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = tokio::time::timeout(
        Duration::from_secs(15),
        declare_scopes(
            &fabric().await,
            notifier_declaration(),
            readiness.clone(),
            fast_config(),
        ),
    )
    .await
    .expect("handshake completed within the timeout")
    .expect("handshake ok");

    assert!(matches!(outcome, ScopeDeclarationOutcome::Accepted));
    assert!(readiness.is_ready(), "duplicate accepted → still just UP");

    teardown(&js).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn disabled_mode_publishes_nothing_and_sets_ready() {
    let Some(_) = common::nats_url() else { return };
    let _guard = serialize_fabric_streams().await;
    let js = jetstream().await;
    create_fabric_streams(&js).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let outcome = declare_scopes(
        &fabric().await,
        notifier_declaration(),
        readiness.clone(),
        ScopeDeclarationConfig::disabled(),
    )
    .await
    .expect("disabled handshake ok");

    assert!(matches!(outcome, ScopeDeclarationOutcome::Disabled));
    assert!(readiness.is_ready(), "disabled → readiness UP");

    tokio::time::sleep(Duration::from_millis(300)).await;
    let count = declare_message_count(&js).await;
    assert_eq!(count, 0, "disabled mode must publish NO declare command");

    teardown(&js).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn missing_stream_fails_loud() {
    let Some(_) = common::nats_url() else { return };
    let _guard = serialize_fabric_streams().await;
    let js = jetstream().await;
    create_fabric_streams(&js).await;
    let _ = js.delete_stream(INTEGRATION_EVT).await;

    let readiness = ReadinessHandle::not_ready("declaring scopes");
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        declare_scopes(
            &fabric().await,
            notifier_declaration(),
            readiness.clone(),
            fast_config(),
        ),
    )
    .await
    .expect("returned without hanging");

    assert!(
        result.is_err(),
        "missing INTEGRATION_EVT stream → fail loud (Err)"
    );
    assert!(!readiness.is_ready(), "fail-loud leaves readiness DOWN");

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}
