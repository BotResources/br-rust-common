//! E2E for the outbox's concurrency + retry semantics against a **real**
//! Postgres + NATS JetStream: multi-replica disjoint drain (`FOR UPDATE SKIP
//! LOCKED`) and the `last_error` reset on an eventual publish. The nominal flow
//! and crash-recovery live in `outbox_e2e.rs`; shared fixtures in `outbox_common`.
#![cfg(feature = "outbox")]

use std::sync::Arc;

use br_core_integration::outbox::{OutboxStore, stage_into};
use br_core_integration::{
    IntegrationEvent, IntegrationPublisher, MessageKind, OutboxRecord, OutboxRelay, RelayPolicy,
    integration_subject,
};
use futures_util::StreamExt;
use uuid::Uuid;

mod outbox_common;
use outbox_common::{
    FlakyPublisher, ThingHappenedV1, connect_pool, create_outbox_table, drop_outbox_table,
    jetstream, read_row, sample_event, setup_stream, unique_prefix, unique_table,
};

/// MULTI-REPLICA DISJOINT DRAIN — `FOR UPDATE SKIP LOCKED` guarantees two
/// concurrent relays over the same `Pending` set never process a row twice.
///
/// Stage N rows, run two relays concurrently against the same table, and assert
/// every row is `PUBLISHED` exactly once and the two relays' `picked` counts
/// partition N (disjoint, no overlap, none missed).
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn two_relays_drain_disjoint_rows() {
    let table = unique_table();
    let pool = connect_pool(8).await;
    create_outbox_table(&pool, &table).await;

    let prefix = unique_prefix();
    let js = jetstream().await;
    let (publisher, consumer, stream_name) = setup_stream(&js, &prefix).await;

    let subject =
        integration_subject(&prefix, MessageKind::Evt, "thing", "happened", 1).expect("subject");
    const N: usize = 20;
    for _ in 0..N {
        let record =
            OutboxRecord::stage_event(Uuid::now_v7(), &subject, &sample_event(Uuid::now_v7()))
                .expect("stage event");
        stage_into(&pool, &table, &record).await.expect("stage row");
    }

    // Two relays over the SAME table + pool, run concurrently.
    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let relay_a = OutboxRelay::with(
        pool.clone(),
        store.clone(),
        publisher.clone(),
        RelayPolicy::default(),
    );
    let relay_b = OutboxRelay::with(
        pool.clone(),
        store.clone(),
        publisher,
        RelayPolicy::default(),
    );
    let (report_a, report_b) = tokio::join!(relay_a.run_once(), relay_b.run_once());
    let report_a = report_a.expect("relay A pass");
    let report_b = report_b.expect("relay B pass");

    // Disjoint partition: the two relays together picked exactly N, neither
    // double-published, and the outbox is fully drained.
    assert_eq!(
        report_a.picked + report_b.picked,
        N,
        "the two relays must partition all rows (a={}, b={})",
        report_a.picked,
        report_b.picked
    );
    assert_eq!(report_a.published + report_b.published, N);
    let drained = store.fetch_pending(&pool, 100).await.expect("re-fetch");
    assert!(drained.is_empty(), "all rows drained");

    // Exactly N distinct messages reached the stream — none published twice.
    let mut messages = consumer.messages().await.expect("messages");
    let mut seen = std::collections::HashSet::new();
    for _ in 0..N {
        let msg = messages.next().await.expect("a message").expect("ok");
        let evt: IntegrationEvent<ThingHappenedV1> =
            serde_json::from_slice(&msg.payload).expect("decode");
        assert!(seen.insert(evt.event_id), "no message published twice");
        msg.ack().await.expect("ack");
    }
    assert_eq!(seen.len(), N);

    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}

/// `last_error` RESET (n1) — a row that fails, records its error, then finally
/// publishes must have `last_error` cleared back to NULL. The column reflects the
/// *latest* attempt, never a stale earlier failure.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn last_error_resets_to_null_on_eventual_publish() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let subject = "thing.happened.v1";
    let row_id = Uuid::now_v7();
    let record = OutboxRecord::stage(row_id, subject, serde_json::json!({"k": "v"}));
    stage_into(&pool, &table, &record).await.expect("stage row");

    // A publisher that fails the first attempt, then succeeds.
    let publisher: Arc<dyn IntegrationPublisher> = Arc::new(FlakyPublisher::new(1));
    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let relay = OutboxRelay::with(pool.clone(), store, publisher, RelayPolicy::default());

    // Pass 1: publish fails → row stays PENDING with last_error recorded.
    let report = relay.run_once().await.expect("pass 1");
    assert_eq!(report.retried, 1);
    let (status, last_error, _) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PENDING");
    assert!(last_error.is_some(), "first failure recorded last_error");

    // Pass 2: publish succeeds → PUBLISHED and last_error reset to NULL.
    let report = relay.run_once().await.expect("pass 2");
    assert_eq!(report.published, 1);
    let (status, last_error, published) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PUBLISHED");
    assert_eq!(last_error, None, "last_error reset on eventual publish");
    assert!(published);

    drop_outbox_table(&pool, &table).await;
}
