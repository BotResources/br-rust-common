#![cfg(feature = "outbox")]

use std::sync::Arc;
use std::time::Duration;

use br_core_integration::outbox::{OutboxStore, stage_into};
use br_core_integration::{
    IntegrationEvent, MessageKind, OutboxRecord, OutboxRelay, RelayHealth, RelayPolicy,
    integration_subject,
};
use futures_util::StreamExt;
use uuid::Uuid;

mod outbox_common;
use outbox_common::{
    ThingHappenedV1, await_health, await_status, connect_pool, create_outbox_table,
    drop_outbox_table, jetstream, nats_publisher, read_row, sample_event, setup_stream,
    unique_prefix, unique_table,
};

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn staging_a_row_wakes_the_running_relay_and_publishes() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let prefix = unique_prefix();
    let js = jetstream().await;
    let (publisher, consumer, stream_name) = setup_stream(&js, &prefix).await;

    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let relay = Arc::new(OutboxRelay::with(
        pool.clone(),
        store,
        publisher,
        RelayPolicy::default(),
    ));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let task = {
        let relay = relay.clone();
        tokio::spawn(async move { relay.run(shutdown_rx).await })
    };

    tokio::time::sleep(Duration::from_millis(200)).await;

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

    assert!(
        await_status(&pool, &table, row_id, "PUBLISHED", Duration::from_secs(5)).await,
        "the NOTIFY must wake the parked relay and the row publishes"
    );

    let mut messages = consumer.messages().await.expect("messages");
    let msg = messages.next().await.expect("a message").expect("ok");
    let received: IntegrationEvent<ThingHappenedV1> =
        serde_json::from_slice(&msg.payload).expect("decode event");
    assert_eq!(received.payload.thing_id, thing_id);
    msg.ack().await.expect("ack");

    let _ = shutdown_tx.send(true);
    let _ = task.await;
    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn a_structural_publish_failure_keeps_the_row_pending_and_degrades_health() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let js = jetstream().await;
    let undeclared_prefix = unique_prefix();
    let publisher = nats_publisher(&js);

    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let relay = Arc::new(OutboxRelay::with(
        pool.clone(),
        store,
        publisher,
        RelayPolicy::default(),
    ));
    let mut health = relay.health();
    assert_eq!(*health.borrow_and_update(), RelayHealth::Healthy);

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let task = {
        let relay = relay.clone();
        tokio::spawn(async move { relay.run(shutdown_rx).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;

    let subject = integration_subject(&undeclared_prefix, MessageKind::Evt, "thing", "happened", 1)
        .expect("subject");
    let row_id = Uuid::now_v7();
    let record =
        OutboxRecord::stage_event(row_id, &subject, &sample_event(Uuid::now_v7())).expect("stage");
    stage_into(&pool, &table, &record).await.expect("stage row");

    assert!(
        await_health(&mut health, Duration::from_secs(5), |h| matches!(
            h,
            RelayHealth::Degraded { .. }
        ))
        .await,
        "a structural NoStream failure must degrade relay health"
    );

    let (status, last_error, published) = read_row(&pool, &table, row_id).await;
    assert_eq!(
        status, "PENDING",
        "a structural fault never marches to Failed"
    );
    assert!(!published, "nothing was published");
    assert!(
        last_error.is_some(),
        "the structural error is recorded for diagnosis"
    );
    let attempts: i64 = sqlx::query_scalar(&format!("SELECT attempts FROM {table} WHERE id = $1"))
        .bind(row_id)
        .fetch_one(&pool)
        .await
        .expect("read attempts");
    assert_eq!(
        attempts, 0,
        "a structural fault does not consume an attempt"
    );

    let _ = shutdown_tx.send(true);
    let _ = task.await;
    drop_outbox_table(&pool, &table).await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn run_drains_a_preexisting_pending_row_on_startup() {
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
    let record =
        OutboxRecord::stage_event(row_id, &subject, &sample_event(thing_id)).expect("stage event");
    stage_into(&pool, &table, &record).await.expect("stage row");

    let (status, _, _) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PENDING", "the row pre-exists, unpublished");

    let store = OutboxStore::new(table.clone()).expect("valid table name");
    let relay = Arc::new(OutboxRelay::with(
        pool.clone(),
        store,
        publisher,
        RelayPolicy::default(),
    ));
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let task = {
        let relay = relay.clone();
        tokio::spawn(async move { relay.run(shutdown_rx).await })
    };

    assert!(
        await_status(&pool, &table, row_id, "PUBLISHED", Duration::from_secs(5)).await,
        "the startup recovery drain must publish the pre-existing Pending row"
    );

    let mut messages = consumer.messages().await.expect("messages");
    let msg = messages.next().await.expect("a message").expect("ok");
    let received: IntegrationEvent<ThingHappenedV1> =
        serde_json::from_slice(&msg.payload).expect("decode event");
    assert_eq!(received.payload.thing_id, thing_id);
    msg.ack().await.expect("ack");

    let _ = shutdown_tx.send(true);
    let _ = task.await;
    drop_outbox_table(&pool, &table).await;
    let _ = js.delete_stream(&stream_name).await;
}
