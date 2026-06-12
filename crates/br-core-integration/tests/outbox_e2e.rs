//! E2E for the transactional outbox against a **real** Postgres + NATS
//! JetStream — the nominal stage→relay→publish flow, the empty-outbox no-op, and
//! the crash-recovery property the outbox *exists* to guarantee. Concurrency and
//! retry semantics live in `outbox_concurrency_e2e.rs`. Shared fixtures and the
//! run-gating doc are in `outbox_common`.
#![cfg(feature = "outbox")]

use br_core_integration::outbox::{OutboxStatus, OutboxStore, stage_into};
use br_core_integration::{
    IntegrationEvent, MessageKind, OutboxRecord, OutboxRelay, RelayPolicy, integration_subject,
};
use futures_util::StreamExt;
use uuid::Uuid;

mod outbox_common;
use outbox_common::{
    ThingHappenedV1, connect_pool, create_outbox_table, drop_outbox_table, jetstream, read_row,
    sample_event, setup_stream, unique_prefix, unique_table,
};

/// The happy path: stage in a transaction with the domain write, run the relay,
/// and the event reaches the stream and the row is `Published`.
#[tokio::test]
#[ignore = "requires DATABASE_URL + NATS_URL (real infra)"]
async fn stage_then_relay_publishes_and_marks_published() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let prefix = unique_prefix();
    let js = jetstream().await;
    let (publisher, consumer, stream_name) = setup_stream(&js, &prefix).await;

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
    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let pending = store.fetch_pending(&pool, 10).await.expect("fetch pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, OutboxStatus::Pending);

    // Run the relay: it publishes the row and marks it PUBLISHED.
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

    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}

/// A `run_once` drain pass over an empty (or already-drained) outbox is a no-op:
/// it finds no `Pending` row and returns immediately, without spinning. (The
/// blessed entry point is the subscribe-driven `run` loop; this proves the drain
/// itself idles cleanly when there is nothing to do.)
#[tokio::test]
#[ignore = "requires DATABASE_URL + NATS_URL (real infra)"]
async fn relay_is_a_noop_on_an_empty_outbox() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let js = jetstream().await;
    let (publisher, _consumer, stream_name) = setup_stream(&js, &unique_prefix()).await;

    let relay = OutboxRelay::with(
        pool.clone(),
        OutboxStore::new(table.clone()).expect("valid table name"),
        publisher,
        RelayPolicy::default(),
    );
    let report = relay.run_once().await.expect("relay pass");
    assert_eq!(report.picked, 0);
    assert_eq!(report.published, 0);

    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}

/// CRASH RECOVERY — the property the outbox *exists* to guarantee.
///
/// Stage a `Pending` row in a committed transaction (the domain write is durable)
/// but never publish it — exactly the state a crash between commit and publish
/// leaves behind. A fresh relay (the startup recovery sweep) must publish it via
/// the **same** code path as the nominal post-commit run.
#[tokio::test]
#[ignore = "requires DATABASE_URL + NATS_URL (real infra)"]
async fn crash_before_publish_recovers_on_next_relay_run() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let prefix = unique_prefix();
    let js = jetstream().await;
    let (publisher, consumer, stream_name) = setup_stream(&js, &prefix).await;

    // Stage with the domain write and commit — then "crash": we do NOT publish.
    let thing_id = Uuid::now_v7();
    let subject =
        integration_subject(&prefix, MessageKind::Evt, "thing", "happened", 1).expect("subject");
    let row_id = Uuid::now_v7();
    let mut tx = pool.begin().await.expect("begin tx");
    let record =
        OutboxRecord::stage_event(row_id, &subject, &sample_event(thing_id)).expect("stage event");
    stage_into(&mut *tx, &table, &record)
        .await
        .expect("stage into outbox");
    tx.commit().await.expect("commit"); // domain write durable; publish never happened

    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let (status, _, published) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PENDING", "the crash left the row unpublished");
    assert!(!published, "published_at must be NULL before recovery");

    // Recovery: a brand-new relay (as at startup) drains the leftover row via the
    // ordinary nominal path — there is no separate recovery code.
    let relay = OutboxRelay::with(pool.clone(), store, publisher, RelayPolicy::default());
    let report = relay.run_once().await.expect("recovery pass");
    assert_eq!(report.picked, 1);
    assert_eq!(report.published, 1);

    let (status, last_error, published) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PUBLISHED");
    assert!(published, "published_at stamped on recovery");
    assert_eq!(last_error, None);

    // The event actually reached the stream.
    let mut messages = consumer.messages().await.expect("messages");
    let msg = messages.next().await.expect("a message").expect("ok");
    let received: IntegrationEvent<ThingHappenedV1> =
        serde_json::from_slice(&msg.payload).expect("decode event");
    assert_eq!(received.payload.thing_id, thing_id);
    msg.ack().await.expect("ack");

    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}
