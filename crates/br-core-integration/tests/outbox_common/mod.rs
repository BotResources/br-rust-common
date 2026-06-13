#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_core_integration::{
    EventMetadata, IntegrationError, IntegrationEvent, IntegrationPublisher,
    NatsIntegrationPublisher, PublishErrorKind,
};
use br_core_kernel::{Actor, UserId};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ThingHappenedV1 {
    pub thing_id: Uuid,
}

pub struct FlakyPublisher {
    fail_first: usize,
    seen: AtomicUsize,
}

impl FlakyPublisher {
    pub fn new(fail_first: usize) -> Self {
        Self {
            fail_first,
            seen: AtomicUsize::new(0),
        }
    }
}

#[async_trait::async_trait]
impl IntegrationPublisher for FlakyPublisher {
    async fn publish(
        &self,
        _subject: &str,
        _payload: serde_json::Value,
    ) -> Result<(), IntegrationError> {
        let n = self.seen.fetch_add(1, Ordering::SeqCst);
        if n < self.fail_first {
            Err(IntegrationError::Publish {
                kind: PublishErrorKind::Other,
                detail: "simulated transient publish failure".to_string(),
            })
        } else {
            Ok(())
        }
    }

    async fn publish_if_connected(&self, _subject: &str, _payload: serde_json::Value) {}
}

pub fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the outbox e2e"))
}

pub fn unique_table() -> String {
    format!("outbox_e2e_{}", Uuid::now_v7().simple())
}

pub async fn connect_pool(max_connections: u32) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(&env("TEST_DATABASE_URL"))
        .await
        .expect("connect to Postgres")
}

pub async fn create_outbox_table(pool: &sqlx::PgPool, table: &str) {
    sqlx::query(&format!(
        "CREATE TABLE {table} (
            id           UUID PRIMARY KEY,
            subject      TEXT NOT NULL,
            payload      JSONB NOT NULL,
            status       TEXT NOT NULL DEFAULT 'PENDING',
            attempts     BIGINT NOT NULL DEFAULT 0,
            last_error   TEXT,
            created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
            published_at TIMESTAMPTZ
        )"
    ))
    .execute(pool)
    .await
    .expect("create outbox table");
}

pub async fn drop_outbox_table(pool: &sqlx::PgPool, table: &str) {
    let _ = sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
        .execute(pool)
        .await;
}

pub async fn read_row(
    pool: &sqlx::PgPool,
    table: &str,
    id: Uuid,
) -> (String, Option<String>, bool) {
    let row: (String, Option<String>, bool) = sqlx::query_as(&format!(
        "SELECT status, last_error, published_at IS NOT NULL FROM {table} WHERE id = $1"
    ))
    .bind(id)
    .fetch_one(pool)
    .await
    .expect("read outbox row");
    (row.0, row.1, row.2)
}

pub async fn await_status(
    pool: &sqlx::PgPool,
    table: &str,
    id: Uuid,
    want: &str,
    deadline: std::time::Duration,
) -> bool {
    let start = std::time::Instant::now();
    loop {
        let (status, _, _) = read_row(pool, table, id).await;
        if status == want {
            return true;
        }
        if start.elapsed() >= deadline {
            return false;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
}

pub async fn await_health<F>(
    rx: &mut tokio::sync::watch::Receiver<br_core_integration::RelayHealth>,
    deadline: std::time::Duration,
    mut pred: F,
) -> bool
where
    F: FnMut(&br_core_integration::RelayHealth) -> bool,
{
    if pred(&rx.borrow_and_update()) {
        return true;
    }
    tokio::time::timeout(deadline, async {
        loop {
            if rx.changed().await.is_err() {
                return false;
            }
            if pred(&rx.borrow_and_update()) {
                return true;
            }
        }
    })
    .await
    .unwrap_or(false)
}

pub fn sample_event(thing_id: Uuid) -> IntegrationEvent<ThingHappenedV1> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "thing.happened",
        1,
        Utc::now(),
        EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), Uuid::now_v7()),
        ThingHappenedV1 { thing_id },
    )
}

pub async fn jetstream() -> async_nats::jetstream::Context {
    let client = async_nats::connect(&env("NATS_URL"))
        .await
        .expect("connect to NATS");
    async_nats::jetstream::new(client)
}

pub fn nats_publisher(js: &async_nats::jetstream::Context) -> Arc<dyn IntegrationPublisher> {
    Arc::new(NatsIntegrationPublisher::new(js.clone()))
}

pub async fn setup_stream(
    js: &async_nats::jetstream::Context,
    prefix: &str,
) -> (
    Arc<dyn IntegrationPublisher>,
    async_nats::jetstream::consumer::Consumer<async_nats::jetstream::consumer::pull::Config>,
    String,
) {
    let stream_name = format!("STREAM_{prefix}");
    let _ = js.delete_stream(&stream_name).await;
    let stream = js
        .create_stream(async_nats::jetstream::stream::Config {
            name: stream_name.clone(),
            subjects: vec![format!("{prefix}.>")],
            ..Default::default()
        })
        .await
        .expect("create stream");
    let consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("reader".to_string()),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            ..Default::default()
        })
        .await
        .expect("create durable consumer");
    let publisher: Arc<dyn IntegrationPublisher> =
        Arc::new(NatsIntegrationPublisher::new(js.clone()));
    (publisher, consumer, stream_name)
}

pub fn unique_prefix() -> String {
    format!("outboxe2e{}", Uuid::now_v7().simple())
}
