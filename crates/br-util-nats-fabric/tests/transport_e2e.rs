use std::time::Duration;

use br_core_integration::{EventMetadata, IntegrationEvent};
use br_core_kernel::{Actor, UserId};
use br_util_nats_fabric::{
    Aggregate, Bc, ConnectionState, EventCoords, Fabric, FabricError, INTEGRATION_EVT, PastFact,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Payload {
    label: String,
}

fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

async fn fabric() -> Fabric {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    Fabric::new(async_nats::jetstream::new(client))
}

async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn recreate_event_stream(js: &async_nats::jetstream::Context, duplicate_window: Duration) {
    let _ = js.delete_stream(INTEGRATION_EVT).await;
    js.create_stream(async_nats::jetstream::stream::Config {
        name: INTEGRATION_EVT.to_string(),
        subjects: vec!["integration.evt.>".to_string()],
        duplicate_window,
        ..Default::default()
    })
    .await
    .expect("create fixed event stream");
}

fn user_created_coords() -> EventCoords {
    EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    }
}

fn group_created_coords() -> EventCoords {
    EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("group").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    }
}

fn event(label: &str) -> IntegrationEvent<Payload> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "fact",
        1,
        Utc::now(),
        EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), Uuid::now_v7()),
        Payload {
            label: label.to_string(),
        },
    )
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn ensure_creates_the_durable_then_fan_in_consumes_both_facts_on_one_consumer() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_event_stream(&js, Duration::from_secs(2)).await;

    let durable = format!("fanin_{}", Uuid::now_v7().simple());
    let user = user_created_coords();
    let group = group_created_coords();
    let fabric = fabric().await;
    fabric
        .publish_event(&user, &event("a-user"))
        .await
        .expect("publish user event");
    fabric
        .publish_event(&group, &event("a-group"))
        .await
        .expect("publish group event");

    let mut consumer = fabric
        .ensure_event_consumer_many::<Payload>(&[&user, &group], &durable)
        .await
        .expect("ensure fan-in durable (lib creates it)");

    let mut seen = Vec::new();
    for _ in 0..2 {
        let delivery = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
            .await
            .expect("recv within deadline")
            .expect("recv ok")
            .expect("a delivery");
        seen.push(delivery.payload().unwrap().payload.label.clone());
        delivery.ack().await.expect("ack");
    }
    seen.sort();
    assert_eq!(seen, vec!["a-group".to_string(), "a-user".to_string()]);

    let after = tokio::time::timeout(Duration::from_secs(2), consumer.recv()).await;
    assert!(
        after.is_err(),
        "both acked facts must not be redelivered on one durable"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn two_ensure_calls_on_the_same_durable_share_one_consumer() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_event_stream(&js, Duration::from_secs(2)).await;

    let durable = format!("shared_{}", Uuid::now_v7().simple());
    let user = user_created_coords();
    let fabric = fabric().await;

    let first = fabric
        .ensure_event_consumer::<Payload>(&user, &durable)
        .await
        .expect("first ensure creates the durable");
    first.drain().await;

    fabric
        .publish_event(&user, &event("shared"))
        .await
        .expect("publish event");

    let mut second = fabric
        .ensure_event_consumer::<Payload>(&user, &durable)
        .await
        .expect("second ensure converges to the same durable, no error");

    let delivery = tokio::time::timeout(Duration::from_secs(5), second.recv())
        .await
        .expect("recv within deadline")
        .expect("recv ok")
        .expect("a delivery from the shared durable");
    assert_eq!(delivery.payload().unwrap().payload.label, "shared");
    delivery.ack().await.expect("ack");

    let stream = js.get_stream(INTEGRATION_EVT).await.unwrap();
    let info = stream.consumer_info(&durable).await.expect("consumer info");
    assert_eq!(
        info.config.filter_subject, "integration.evt.identity.user.created.v1",
        "the lib's config is authoritative for the durable's filter"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn two_concurrent_ensure_calls_on_the_same_durable_converge_to_one_consumer() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_event_stream(&js, Duration::from_secs(2)).await;

    let durable = format!("concurrent_{}", Uuid::now_v7().simple());
    let user = user_created_coords();
    let fabric = fabric().await;

    let (first, second) = tokio::join!(
        fabric.ensure_event_consumer::<Payload>(&user, &durable),
        fabric.ensure_event_consumer::<Payload>(&user, &durable),
    );
    let first = first.expect("first concurrent ensure ok");
    let second = second.expect("second concurrent ensure ok");
    first.drain().await;
    second.drain().await;

    let stream = js.get_stream(INTEGRATION_EVT).await.unwrap();
    let mut consumers = stream.consumers();
    let mut count = 0usize;
    while let Some(consumer) = futures_util::StreamExt::next(&mut consumers).await {
        let consumer = consumer.expect("consumer info");
        assert_eq!(
            consumer.name, durable,
            "the only consumer on the stream is the shared durable"
        );
        count += 1;
    }
    assert_eq!(
        count, 1,
        "two concurrent ensure calls on the same durable + identical config converge to exactly one broker consumer (multi-pod overlap is safe)"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn ensure_rejects_an_empty_coordinate_set() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_event_stream(&js, Duration::from_secs(2)).await;

    let durable = format!("empty_{}", Uuid::now_v7().simple());
    let fabric = fabric().await;
    let result = fabric
        .ensure_event_consumer_many::<Payload>(&[], &durable)
        .await;
    assert!(matches!(
        result.err(),
        Some(FabricError::FilterMismatch { .. })
    ));

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn graceful_drain_acks_the_in_flight_message_and_stops_without_redelivery() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_event_stream(&js, Duration::from_secs(2)).await;

    let durable = format!("drain_{}", Uuid::now_v7().simple());
    let user = user_created_coords();
    let fabric = fabric().await;
    fabric
        .publish_event(&user, &event("drain-me"))
        .await
        .expect("publish event");

    let mut consumer = fabric
        .ensure_event_consumer::<Payload>(&user, &durable)
        .await
        .expect("ensure durable");

    let delivery = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("recv within deadline")
        .expect("recv ok")
        .expect("a delivery");
    assert_eq!(delivery.payload().unwrap().payload.label, "drain-me");
    delivery.ack().await.expect("ack in-flight before drain");

    consumer.drain().await;

    let mut rebound = fabric
        .ensure_event_consumer::<Payload>(&user, &durable)
        .await
        .expect("re-ensure durable after drain");
    let after = tokio::time::timeout(Duration::from_secs(2), rebound.recv()).await;
    assert!(
        after.is_err(),
        "the acked in-flight message must not be redelivered after a graceful drain"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn graceful_drain_leaves_an_unacked_frame_held_under_ack_wait() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_event_stream(&js, Duration::from_secs(2)).await;

    let durable = format!("drain_unacked_{}", Uuid::now_v7().simple());
    let user = user_created_coords();
    let fabric = fabric().await;
    fabric
        .publish_event(&user, &event("held-me"))
        .await
        .expect("publish event");

    let mut consumer = fabric
        .ensure_event_consumer::<Payload>(&user, &durable)
        .await
        .expect("ensure durable");

    let delivery = tokio::time::timeout(Duration::from_secs(5), consumer.recv())
        .await
        .expect("recv within deadline")
        .expect("recv ok")
        .expect("a delivery");
    assert_eq!(delivery.payload().unwrap().payload.label, "held-me");
    drop(delivery);

    consumer.drain().await;

    let mut rebound = fabric
        .ensure_event_consumer::<Payload>(&user, &durable)
        .await
        .expect("re-ensure durable after drain");
    let within_ack_wait = tokio::time::timeout(Duration::from_secs(3), rebound.recv()).await;
    assert!(
        within_ack_wait.is_err(),
        "an un-acked frame is held in-flight under the lib's 30s ack_wait, not redelivered immediately (at-least-once preserved, no silent loss)"
    );

    let redelivered = tokio::time::timeout(Duration::from_secs(35), rebound.recv())
        .await
        .expect("the un-acked frame is redelivered past the 30s ack_wait")
        .expect("recv ok")
        .expect("a redelivered delivery");
    assert_eq!(
        redelivered.payload().unwrap().payload.label,
        "held-me",
        "the un-acked frame IS redelivered after ack_wait elapses (at-least-once guarantee)"
    );
    redelivered.ack().await.expect("ack the redelivered frame");

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn idempotent_publish_dedups_the_same_message_id_within_the_window() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_event_stream(&js, Duration::from_secs(120)).await;

    let user = user_created_coords();
    let fabric = fabric().await;
    let message_id = Uuid::now_v7().to_string();

    fabric
        .publish_event_with_id(&user, &event("once"), &message_id)
        .await
        .expect("first idempotent publish");
    fabric
        .publish_event_with_id(&user, &event("once"), &message_id)
        .await
        .expect("second idempotent publish (deduped)");

    let mut stream = js.get_stream(INTEGRATION_EVT).await.unwrap();
    let info = stream.info().await.expect("stream info");
    assert_eq!(
        info.state.messages, 1,
        "the duplicate Nats-Msg-Id must be deduped to a single stored message"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn reachable_reports_connected_against_a_live_broker() {
    let Some(_) = nats_url() else { return };
    let fabric = fabric().await;
    assert_eq!(fabric.connection_state(), ConnectionState::Connected);
    assert!(fabric.reachable());
    fabric
        .ping()
        .await
        .expect("round-trip flush against a live broker");
}
