use std::time::Duration;

use br_core_integration::{EventMetadata, IntegrationCommand, IntegrationEvent, MessageOutcome};
use br_core_kernel::{Actor, UserId};
use br_util_nats_fabric::{
    Aggregate, Bc, CommandCoords, EventCoords, Fabric, FabricError, INTEGRATION_CMD,
    INTEGRATION_EVT, KV_PUBLISHED_LANGUAGE, PastFact, PublishedLanguagePublisher, Verb,
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

async fn recreate_stream(js: &async_nats::jetstream::Context, name: &str, bind: &str) {
    let _ = js.delete_stream(name).await;
    js.create_stream(async_nats::jetstream::stream::Config {
        name: name.to_string(),
        subjects: vec![bind.to_string()],
        ..Default::default()
    })
    .await
    .expect("create fixed stream");
}

fn command(label: &str, correlation_id: Uuid) -> IntegrationCommand<Payload> {
    IntegrationCommand::new(
        Uuid::now_v7(),
        "notification.deliver",
        1,
        Utc::now(),
        EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id),
        Payload {
            label: label.to_string(),
        },
    )
}

fn event(label: &str, correlation_id: Uuid) -> IntegrationEvent<Payload> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "user.created",
        1,
        Utc::now(),
        EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id),
        Payload {
            label: label.to_string(),
        },
    )
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn command_renders_grammar_and_a_matching_durable_consumes_it() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_CMD, "integration.cmd.>").await;

    let coords = CommandCoords {
        receiver: Bc::new("notifier").unwrap(),
        aggregate: Aggregate::new("notification").unwrap(),
        verb: Verb::new("deliver").unwrap(),
        version: 1,
    };
    let durable = format!("test_{}", Uuid::now_v7().simple());
    let stream = js.get_stream(INTEGRATION_CMD).await.unwrap();
    stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some(durable.clone()),
            filter_subject: "integration.cmd.notifier.notification.deliver.v1".to_string(),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            ..Default::default()
        })
        .await
        .unwrap();

    let fabric = fabric().await;
    let correlation = Uuid::now_v7();
    fabric
        .publish_command(&coords, &command("hello", correlation))
        .await
        .expect("publish command");

    let received = tokio::time::timeout(Duration::from_secs(5), async {
        fabric
            .run_commands::<Payload, _, _, _>(
                &coords,
                &durable,
                |delivery| async move {
                    assert_eq!(delivery.envelope.payload.label, "hello");
                    MessageOutcome::Ack
                },
                |_| {},
            )
            .await
    })
    .await;
    assert!(received.is_err() || received.unwrap().is_ok());

    let _ = js.delete_stream(INTEGRATION_CMD).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn a_widened_durable_is_rejected_on_bind() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_EVT, "integration.evt.>").await;

    let durable = format!("wide_{}", Uuid::now_v7().simple());
    let stream = js.get_stream(INTEGRATION_EVT).await.unwrap();
    stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some(durable.clone()),
            filter_subject: "integration.evt.>".to_string(),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            ..Default::default()
        })
        .await
        .unwrap();

    let coords = EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    };
    let fabric = fabric().await;
    let err = fabric
        .verify_event_durable(&coords, &durable)
        .await
        .unwrap_err();
    assert!(matches!(err, FabricError::FilterMismatch { .. }));

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn awaiter_matches_by_correlation_id() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    recreate_stream(&js, INTEGRATION_EVT, "integration.evt.>").await;

    let coords = EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    };
    let fabric = fabric().await;
    let mut awaiter = fabric.await_event(&coords).await.expect("await_event");

    let correlation = Uuid::now_v7();
    fabric
        .publish_event(&coords, &event("evt", correlation))
        .await
        .expect("publish event");

    let matched = awaiter
        .await_correlation(correlation, Duration::from_secs(5))
        .await
        .expect("await_correlation");
    assert!(matched.is_some());

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn published_language_binds_existing_bucket_and_fails_loud_when_absent() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let _ = js.delete_key_value(KV_PUBLISHED_LANGUAGE).await;

    let fabric = fabric().await;
    let absent = PublishedLanguagePublisher::<Payload>::open(&fabric).await;
    assert!(matches!(absent, Err(FabricError::Kv(_))));

    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_PUBLISHED_LANGUAGE.to_string(),
        ..Default::default()
    })
    .await
    .expect("create bucket");
    assert!(
        PublishedLanguagePublisher::<Payload>::open(&fabric)
            .await
            .is_ok()
    );

    let _ = js.delete_key_value(KV_PUBLISHED_LANGUAGE).await;
}
