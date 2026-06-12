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

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
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

    let mut tx = pool.begin().await.expect("begin tx");
    let record = OutboxRecord::stage_event(Uuid::now_v7(), &subject, &event).expect("stage event");
    stage_into(&mut *tx, &table, &record)
        .await
        .expect("stage into outbox");
    tx.commit().await.expect("commit");

    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let pending = store.fetch_pending(&pool, 10).await.expect("fetch pending");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, OutboxStatus::Pending);

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

    let still_pending = store.fetch_pending(&pool, 10).await.expect("re-fetch");
    assert!(still_pending.is_empty(), "row should be drained");

    let mut messages = consumer.messages().await.expect("messages");
    let msg = messages.next().await.expect("a message").expect("ok");
    let received: IntegrationEvent<ThingHappenedV1> =
        serde_json::from_slice(&msg.payload).expect("decode event");
    assert_eq!(received.payload.thing_id, thing_id);
    msg.ack().await.expect("ack");

    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
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

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn crash_before_publish_recovers_on_next_relay_run() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let prefix = unique_prefix();
    let js = jetstream().await;
    let (publisher, consumer, stream_name) = setup_stream(&js, &prefix).await;

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
    tx.commit().await.expect("commit");

    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let (status, _, published) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PENDING", "the crash left the row unpublished");
    assert!(!published, "published_at must be NULL before recovery");

    let relay = OutboxRelay::with(pool.clone(), store, publisher, RelayPolicy::default());
    let report = relay.run_once().await.expect("recovery pass");
    assert_eq!(report.picked, 1);
    assert_eq!(report.published, 1);

    let (status, last_error, published) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PUBLISHED");
    assert!(published, "published_at stamped on recovery");
    assert_eq!(last_error, None);

    let mut messages = consumer.messages().await.expect("messages");
    let msg = messages.next().await.expect("a message").expect("ok");
    let received: IntegrationEvent<ThingHappenedV1> =
        serde_json::from_slice(&msg.payload).expect("decode event");
    assert_eq!(received.payload.thing_id, thing_id);
    msg.ack().await.expect("ack");

    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}
