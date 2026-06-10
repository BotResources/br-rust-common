//! E2E for [`declare_scopes`] against a real NATS JetStream broker, with a
//! **stub receiver implemented in this test** standing in for Identity (it
//! subscribes to the declare subject and replies accepted/rejected, echoing the
//! `correlation_id`). Proves the handshake end to end: Accepted, Rejected,
//! timeout→re-publish→Accepted, disabled (no publish), duplicate-confirmation
//! (first match wins) — and the readiness gate state after each.
//!
//! See `tests/common/mod.rs` for gating (`#[ignore]`, `NATS_URL`,
//! `--test-threads=1`, unique stream per test).

mod common;

use std::time::Duration;

use br_util_axum_readiness::ReadinessHandle;
use br_util_scope_declaration::{ScopeDeclarationConfig, ScopeDeclarationOutcome, declare_scopes};
use common::{
    StubReceiver, StubReply, create_identity_stream, declare_message_count, jetstream,
    notifier_declaration, spawn_delayed_accept_stub, teardown, unique_stream,
};

/// A short wait timeout so the timeout→re-publish path is fast in tests.
fn fast_config(stream_name: &str) -> ScopeDeclarationConfig {
    let mut config = ScopeDeclarationConfig::enabled(stream_name);
    config.wait_timeout = Duration::from_millis(500);
    config
}

/// Accepted: the stub replies on the accepted subject → the helper returns
/// `Accepted` and the readiness gate is UP.
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

/// Rejected: the stub replies on the rejected subject → the helper returns
/// `Rejected` carrying the structured reason, and the gate is DOWN (no retry).
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
            // codes-not-language: the reason carries a stable code.
            assert_eq!(reason.reason.to_string(), "scope_owned_by_another_service");
            assert_eq!(reason.service.as_str(), "notifier");
        }
        other => panic!("expected Rejected, got {other:?}"),
    }
    assert!(!readiness.is_ready(), "rejected → readiness DOWN");

    teardown(&js, &stream).await;
}

/// Timeout → re-publish → Accepted: the stub ignores the first command (the
/// helper times out and re-publishes the same correlation_id), then accepts the
/// second → the helper resolves `Accepted` and the gate is UP.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn timeout_then_republish_then_accepted() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let stream = unique_stream();
    let _s = create_identity_stream(&js, &stream).await;
    // Swallow the first declare → force one timeout + re-publish before accepting.
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

/// Duplicate confirmations: the stub emits the accepted reply twice per command
/// (mimicking timeout-republish + always-re-emit). First match wins; the helper
/// resolves `Accepted` once and the extra confirmation is harmless.
#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn duplicate_confirmations_first_match_wins() {
    let Some(_) = common::nats_url() else { return };
    let js = jetstream().await;
    let stream = unique_stream();
    let _s = create_identity_stream(&js, &stream).await;
    // Two identical accepted confirmations per declare.
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

/// Disabled mode: the helper publishes NOTHING and awaits nothing — it returns
/// `Disabled` and sets the gate UP. We assert no message was captured on the
/// declare subject. No stub is spawned; there is nothing to reply to.
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

    // Give any (erroneous) publish a moment to land, then assert nothing did.
    tokio::time::sleep(Duration::from_millis(300)).await;
    let count = declare_message_count(&js, &stream).await;
    assert_eq!(count, 0, "disabled mode must publish NO declare command");

    teardown(&js, &stream).await;
}

/// Fail-loud: an enabled handshake against a missing stream returns an error
/// (the awaiter binds the stream by name and never creates it) — the gate stays
/// DOWN. We use a short wait so even if it somehow looped, the test timeout
/// would still catch a hang; here it must return `Err` immediately.
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
