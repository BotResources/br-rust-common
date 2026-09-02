use std::time::Duration;

use br_core_integration::{EventMetadata, IntegrationEvent};
use br_core_kernel::{Actor, UserId};
use br_util_nats_fabric::{
    Aggregate, Bc, EventCoords, Fabric, INTEGRATION_EVT, OutboxRecord, OutboxRelay, PastFact, stage,
};
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

const DUPLICATE_WINDOW: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct UserCreatedV1 {
    user_id: Uuid,
}

fn nats_url() -> String {
    std::env::var("NATS_URL").expect("NATS_URL must point at a JetStream-enabled broker")
}

fn database_url() -> String {
    std::env::var("TEST_DATABASE_URL").expect("TEST_DATABASE_URL must point at a Postgres database")
}

async fn jetstream() -> async_nats::jetstream::Context {
    let client = async_nats::connect(&nats_url())
        .await
        .expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn recreate_event_stream(js: &async_nats::jetstream::Context) {
    let _ = js.delete_stream(INTEGRATION_EVT).await;
    js.create_stream(async_nats::jetstream::stream::Config {
        name: INTEGRATION_EVT.to_string(),
        subjects: vec!["integration.evt.>".to_string()],
        duplicate_window: DUPLICATE_WINDOW,
        ..Default::default()
    })
    .await
    .expect("the fixture, never the lib, declares the stream and its duplicate window");
}

async fn outbox_pool() -> PgPool {
    let pool = PgPool::connect(&database_url())
        .await
        .expect("connect to Postgres");
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS integration_outbox ( \
            id UUID PRIMARY KEY, \
            subject TEXT NOT NULL, \
            payload JSONB NOT NULL, \
            status TEXT NOT NULL, \
            attempts BIGINT NOT NULL DEFAULT 0, \
            last_error TEXT, \
            published_at TIMESTAMPTZ, \
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW() \
         )",
    )
    .execute(&pool)
    .await
    .expect("the outbox DDL is consumer-owned, so the fixture creates it");
    sqlx::query("TRUNCATE integration_outbox")
        .execute(&pool)
        .await
        .expect("truncate the outbox");
    pool
}

async fn rewind_to_pending(pool: &PgPool, id: Uuid) {
    sqlx::query(
        "UPDATE integration_outbox \
         SET status = 'PENDING', attempts = 0, last_error = NULL, published_at = NULL \
         WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await
    .expect("rewind the row to the state a crash between publish and mark leaves behind");
}

async fn row_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM integration_outbox WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read the outbox row status")
}

async fn delivered(js: &async_nats::jetstream::Context) -> Vec<async_nats::jetstream::Message> {
    let stream = js.get_stream(INTEGRATION_EVT).await.expect("stream exists");
    let consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config::default())
        .await
        .expect("an ephemeral raw consumer observes what the stream really holds");
    let mut messages = consumer
        .fetch()
        .max_messages(16)
        .messages()
        .await
        .expect("fetch every stored frame");
    let mut collected = Vec::new();
    while let Some(message) = messages.next().await {
        collected.push(message.expect("a stored frame"));
    }
    collected
}

fn message_id(message: &async_nats::jetstream::Message) -> String {
    message
        .headers
        .as_ref()
        .and_then(|h| h.get(async_nats::header::NATS_MESSAGE_ID))
        .map(|v| v.to_string())
        .expect("the relay sets Nats-Msg-Id on every published frame")
}

fn user_created() -> EventCoords {
    EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new("created").unwrap(),
        version: 1,
    }
}

fn envelope(event_id: Uuid) -> IntegrationEvent<UserCreatedV1> {
    IntegrationEvent::new(
        event_id,
        "user.created",
        1,
        Utc::now(),
        EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), Uuid::now_v7()),
        UserCreatedV1 {
            user_id: Uuid::now_v7(),
        },
    )
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and database"]
async fn a_crash_between_publish_and_mark_replays_the_row_and_the_broker_dedups_it() {
    let js = jetstream().await;
    recreate_event_stream(&js).await;
    let pool = outbox_pool().await;
    let relay = OutboxRelay::new(pool.clone(), Fabric::new(jetstream().await));

    let row_id = Uuid::now_v7();
    let event_id = Uuid::now_v7();
    let record =
        OutboxRecord::stage_event(row_id, user_created(), &envelope(event_id)).expect("stage");
    stage(&pool, &record).await.expect("persist the staged row");

    let first = relay.run_once_detailed().await.expect("first relay pass");
    assert_eq!(first.picked, 1);
    assert_eq!(first.published, 1);
    assert_eq!(first.duplicates, 0);
    assert_eq!(
        first.row_id_fallbacks, 0,
        "the envelope carries an event_id"
    );
    assert_eq!(row_status(&pool, row_id).await, "PUBLISHED");

    rewind_to_pending(&pool, row_id).await;

    let replay = relay.run_once_detailed().await.expect("replay pass");
    assert_eq!(replay.picked, 1);
    assert_eq!(
        replay.published, 1,
        "the broker accepted the frame, so the row is marked published"
    );
    assert_eq!(
        replay.duplicates, 1,
        "the replay must be visible as a duplicate ack, never silently absorbed"
    );

    let stored = delivered(&js).await;
    assert_eq!(
        stored.len(),
        1,
        "the duplicate window collapses the replay to a single delivery"
    );
    assert_eq!(
        stored[0].subject.as_str(),
        "integration.evt.identity.user.created.v1"
    );
    assert_eq!(
        message_id(&stored[0]),
        event_id.to_string(),
        "the dedup key is the envelope event_id, not the outbox row id"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and database"]
async fn a_replay_after_the_duplicate_window_is_delivered_twice_because_delivery_is_at_least_once()
{
    let js = jetstream().await;
    recreate_event_stream(&js).await;
    let pool = outbox_pool().await;
    let relay = OutboxRelay::new(pool.clone(), Fabric::new(jetstream().await));

    let row_id = Uuid::now_v7();
    let event_id = Uuid::now_v7();
    let record =
        OutboxRecord::stage_event(row_id, user_created(), &envelope(event_id)).expect("stage");
    stage(&pool, &record).await.expect("persist the staged row");

    let first = relay.run_once_detailed().await.expect("first relay pass");
    assert_eq!(first.published, 1);
    assert_eq!(first.duplicates, 0);

    rewind_to_pending(&pool, row_id).await;
    tokio::time::sleep(DUPLICATE_WINDOW + Duration::from_secs(1)).await;

    let replay = relay.run_once_detailed().await.expect("replay pass");
    assert_eq!(replay.published, 1);
    assert_eq!(
        replay.duplicates, 0,
        "outside the window the broker stores the frame again"
    );

    let stored = delivered(&js).await;
    assert_eq!(
        stored.len(),
        2,
        "delivery stays at-least-once: the dedup id is a window, not a guarantee"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and database"]
async fn a_payload_without_an_envelope_falls_back_to_the_row_id_and_still_publishes_once() {
    let js = jetstream().await;
    recreate_event_stream(&js).await;
    let pool = outbox_pool().await;
    let relay = OutboxRelay::new(pool.clone(), Fabric::new(jetstream().await));

    let row_id = Uuid::now_v7();
    let record = OutboxRecord::stage(
        row_id,
        user_created(),
        serde_json::json!({ "raw": "no envelope here" }),
    );
    stage(&pool, &record).await.expect("persist the staged row");

    let pass = relay.run_once_detailed().await.expect("relay pass");
    assert_eq!(pass.published, 1);
    assert_eq!(pass.duplicates, 0);
    assert_eq!(
        pass.row_id_fallbacks, 1,
        "a raw stage has no envelope id, so the row id is the dedup key"
    );

    let stored = delivered(&js).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(message_id(&stored[0]), row_id.to_string());

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}
