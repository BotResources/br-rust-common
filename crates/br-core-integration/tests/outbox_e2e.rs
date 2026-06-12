//! E2E for the transactional outbox against a **real** Postgres + NATS
//! JetStream. Unit tests cover the pure state machine and the relay's
//! bookkeeping; only this suite exercises what bites in production: the
//! same-transaction insert, the post-commit publish reaching the stream, and
//! the status transition persisting.
//!
//! Run gating (matches the other e2e suites — no infra mocks):
//!   - compiled only with `--features outbox`;
//!   - `#[ignore]` by default, opted into via `cargo test -- --ignored`;
//!   - requires BOTH env vars, else the test panics loudly (never a silent
//!     mock-fakes-a-pass): `DATABASE_URL` (a Postgres the test may create a
//!     temp table in) and `NATS_URL` (a JetStream-enabled broker).
//!
//!   docker run -d --rm -p 4222:4222 nats:2-alpine -js
//!   docker run -d --rm -p 5432:5432 -e POSTGRES_PASSWORD=pg postgres:16
//!   DATABASE_URL=postgres://postgres:pg@localhost:5432/postgres \
//!   NATS_URL=nats://localhost:4222 \
//!   cargo test -p br-core-integration --features outbox --test outbox_e2e -- --ignored
#![cfg(feature = "outbox")]

use std::sync::Arc;

use br_core_integration::outbox::{OutboxStatus, OutboxStore, stage_into};
use br_core_integration::{
    IntegrationEvent, IntegrationPublisher, MessageKind, MessageMetadata, NatsIntegrationPublisher,
    OutboxRecord, OutboxRelay, RelayPolicy, integration_subject,
};
use br_core_kernel::{Actor, UserId};
use chrono::Utc;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct ThingHappenedV1 {
    thing_id: Uuid,
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} must be set for the outbox e2e"))
}

/// A unique per-run table so parallel runs don't collide; dropped on teardown.
fn unique_table() -> String {
    format!("outbox_e2e_{}", Uuid::now_v7().simple())
}

async fn create_outbox_table(pool: &sqlx::PgPool, table: &str) {
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

async fn drop_outbox_table(pool: &sqlx::PgPool, table: &str) {
    let _ = sqlx::query(&format!("DROP TABLE IF EXISTS {table}"))
        .execute(pool)
        .await;
}

fn sample_event(subject_thing: Uuid) -> IntegrationEvent<ThingHappenedV1> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        "thing.happened",
        1,
        Utc::now(),
        MessageMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), Uuid::now_v7()),
        ThingHappenedV1 {
            thing_id: subject_thing,
        },
    )
}

/// The happy path: stage in a transaction with the domain write, run the relay,
/// and the event reaches the stream and the row is `Published`.
#[tokio::test]
#[ignore = "requires DATABASE_URL + NATS_URL (real infra)"]
async fn stage_then_relay_publishes_and_marks_published() {
    let table = unique_table();
    let pool = PgPoolOptions::new()
        .connect(&env("DATABASE_URL"))
        .await
        .expect("connect to Postgres");
    create_outbox_table(&pool, &table).await;

    // NATS: a stream capturing the test subject, with a durable consumer we read.
    let prefix = format!("outboxe2e{}", Uuid::now_v7().simple());
    let client = async_nats::connect(&env("NATS_URL"))
        .await
        .expect("connect to NATS");
    let js = async_nats::jetstream::new(client);
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

    let thing_id = Uuid::now_v7();
    let subject =
        integration_subject(&prefix, MessageKind::Evt, "thing", "happened", 1).expect("subject");
    let event = sample_event(thing_id);

    // Stage the outbox row in the SAME transaction as a (here, illustrative)
    // domain write — the atomicity the outbox exists for.
    let mut tx = pool.begin().await.expect("begin tx");
    let record = OutboxRecord::stage_event(Uuid::now_v7(), &subject, &event).expect("stage event");
    stage_into(&mut *tx, &table, &record)
        .await
        .expect("stage into outbox");
    tx.commit().await.expect("commit");

    // The row is PENDING and nothing is on the stream yet (publish is post-commit).
    let store = OutboxStore::new(table.clone());
    let pending = store.fetch_pending(&pool, 10).await.expect("fetch pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, OutboxStatus::Pending);

    // Run the relay: it publishes the row and marks it PUBLISHED.
    let publisher: Arc<dyn IntegrationPublisher> =
        Arc::new(NatsIntegrationPublisher::new(js.clone()));
    let relay = OutboxRelay::with(
        pool.clone(),
        store.clone(),
        publisher,
        RelayPolicy::default(),
    );
    let report = relay.run_once().await.expect("relay pass");
    assert_eq!(report.picked, 1);
    assert_eq!(report.published, 1);
    assert_eq!(report.failed, 0);

    // The outbox row is now PUBLISHED and no longer pending.
    let still_pending = store.fetch_pending(&pool, 10).await.expect("re-fetch");
    assert!(still_pending.is_empty(), "row should be drained");

    // The event landed on the stream and round-trips.
    let mut messages = consumer.messages().await.expect("messages");
    let msg = messages.next().await.expect("a message").expect("ok");
    let received: IntegrationEvent<ThingHappenedV1> =
        serde_json::from_slice(&msg.payload).expect("decode event");
    assert_eq!(received.payload.thing_id, thing_id);
    msg.ack().await.expect("ack");

    // Teardown.
    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}

/// A second relay pass over an already-drained outbox is a no-op — the relay is
/// safe to run on a schedule.
#[tokio::test]
#[ignore = "requires DATABASE_URL + NATS_URL (real infra)"]
async fn relay_is_a_noop_on_an_empty_outbox() {
    let table = unique_table();
    let pool = PgPoolOptions::new()
        .connect(&env("DATABASE_URL"))
        .await
        .expect("connect to Postgres");
    create_outbox_table(&pool, &table).await;

    let client = async_nats::connect(&env("NATS_URL"))
        .await
        .expect("connect to NATS");
    let js = async_nats::jetstream::new(client);
    let publisher: Arc<dyn IntegrationPublisher> = Arc::new(NatsIntegrationPublisher::new(js));

    let relay = OutboxRelay::with(
        pool.clone(),
        OutboxStore::new(table.clone()),
        publisher,
        RelayPolicy::default(),
    );
    let report = relay.run_once().await.expect("relay pass");
    assert_eq!(report.picked, 0);
    assert_eq!(report.published, 0);

    drop_outbox_table(&pool, &table).await;
}
