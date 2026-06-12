//! Shared fixtures for the transactional-outbox e2e suites against **real**
//! Postgres + NATS JetStream (no infra mocks).
//!
//! Gating (matches the other e2e suites):
//!   - compiled only with `--features outbox` (the suites set `#![cfg(...)]`);
//!   - the tests are `#[ignore]` by default, opted into with `--ignored`;
//!   - both env vars must be set, else a loud panic (never a silent fake-pass):
//!     `DATABASE_URL` (a Postgres the test may create a temp table in) and
//!     `NATS_URL` (a JetStream-enabled broker).
//!
//!   docker run -d --rm -p 4222:4222 nats:2-alpine -js
//!   docker run -d --rm -p 5432:5432 -e POSTGRES_PASSWORD=pg postgres:16
//!   DATABASE_URL=postgres://postgres:pg@localhost:5432/postgres \
//!   NATS_URL=nats://localhost:4222 \
//!   cargo test -p br-core-integration --features outbox -- --ignored
//!
//! Each suite includes this module and uses a subset of the helpers, so
//! `dead_code` is expected and silenced (the standard shared-test-helper pattern).
#![allow(dead_code)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use br_core_integration::{
    IntegrationError, IntegrationEvent, IntegrationPublisher, MessageMetadata,
    NatsIntegrationPublisher, PublishErrorKind,
};
use br_core_kernel::{Actor, UserId};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// The integration payload the outbox e2e suites stage and publish.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ThingHappenedV1 {
    pub thing_id: Uuid,
}

/// A publisher that fails its first `fail_first` publishes, then succeeds — used
/// to exercise the retry path and the `last_error` reset. Counts publishes so a
/// test can assert how many attempts the relay made.
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

/// A required env var, or a loud panic — an absent var must never silently skip.
pub fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the outbox e2e"))
}

/// A unique per-run table so parallel runs don't collide; dropped on teardown.
pub fn unique_table() -> String {
    format!("outbox_e2e_{}", Uuid::now_v7().simple())
}

/// Connect a Postgres pool to `DATABASE_URL` with `max_connections` (the
/// concurrency suite needs several; the nominal one is fine with the default).
pub async fn connect_pool(max_connections: u32) -> sqlx::PgPool {
    sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(&env("DATABASE_URL"))
        .await
        .expect("connect to Postgres")
}

/// Create a fresh outbox table matching the canonical DDL (the contract the
/// store binds to). The lib never auto-provisions; the test owns its table.
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

/// Read one row's `(status, last_error, published_at IS NOT NULL)` for
/// assertions. `published_at` is projected to a bool in SQL so the test needs no
/// `sqlx/chrono` feature (the `outbox` feature only pulls `sqlx/uuid` + `json`).
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

pub fn sample_event(thing_id: Uuid) -> IntegrationEvent<ThingHappenedV1> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "thing.happened",
        1,
        Utc::now(),
        MessageMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), Uuid::now_v7()),
        ThingHappenedV1 { thing_id },
    )
}

/// A connected NATS JetStream context for `NATS_URL`.
pub async fn jetstream() -> async_nats::jetstream::Context {
    let client = async_nats::connect(&env("NATS_URL"))
        .await
        .expect("connect to NATS");
    async_nats::jetstream::new(client)
}

/// Set up a JetStream stream capturing `{prefix}.>` with a durable reader, and
/// return the publisher + consumer + stream name (for teardown). Every infra
/// test exercises a real broker through this fixture.
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

/// A unique per-run subject prefix for an outbox e2e stream.
pub fn unique_prefix() -> String {
    format!("outboxe2e{}", Uuid::now_v7().simple())
}
