use std::time::Duration;

use br_core_integration::{EventMetadata, IntegrationEvent};
use br_core_kernel::{Actor, UserId};
use br_test_support::{require_nats_url, require_test_db_url};
use br_util_nats_fabric::{Aggregate, Bc, EventCoords, INTEGRATION_EVT, PastFact};
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

pub const DUPLICATE_WINDOW: Duration = Duration::from_secs(2);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct UserCreatedV1 {
    user_id: Uuid,
}

pub async fn jetstream() -> async_nats::jetstream::Context {
    let client = async_nats::connect(&require_nats_url())
        .await
        .expect("connect to NATS");
    async_nats::jetstream::new(client)
}

pub async fn recreate_event_stream(js: &async_nats::jetstream::Context) {
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

pub async fn outbox_pool() -> PgPool {
    let pool = PgPool::connect(&require_test_db_url())
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

pub async fn rewind_to_pending(pool: &PgPool, id: Uuid) {
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

pub async fn row_status(pool: &PgPool, id: Uuid) -> String {
    sqlx::query_scalar("SELECT status FROM integration_outbox WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
        .expect("read the outbox row status")
}

pub async fn delivered(js: &async_nats::jetstream::Context) -> Vec<async_nats::jetstream::Message> {
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

pub fn message_id(message: &async_nats::jetstream::Message) -> String {
    message
        .headers
        .as_ref()
        .and_then(|h| h.get(async_nats::header::NATS_MESSAGE_ID))
        .map(|v| v.to_string())
        .expect("the relay sets Nats-Msg-Id on every published frame")
}

pub fn coords(fact: &str) -> EventCoords {
    EventCoords {
        producer: Bc::new("identity").unwrap(),
        aggregate: Aggregate::new("user").unwrap(),
        fact: PastFact::new(fact).unwrap(),
        version: 1,
    }
}

pub fn user_created() -> EventCoords {
    coords("created")
}

pub fn envelope(event_id: Uuid) -> IntegrationEvent<UserCreatedV1> {
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
