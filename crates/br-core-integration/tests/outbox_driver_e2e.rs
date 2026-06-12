//! E2E for the **subscribe-driven** relay loop ([`OutboxRelay::run`]) against a
//! **real** Postgres + NATS JetStream — the properties the `run()` entry point
//! adds over the bare `run_once` drain:
//!
//! - (a) staging a row fires a `NOTIFY` that **wakes** a parked `run()` relay and
//!   the row publishes, with **no** polling involved (the relay is woken by the
//!   commit, not by a timer);
//! - (b) a **structural** publish error (publish to an undeclared stream) leaves
//!   the row `Pending` with its **attempt not consumed**, and flips the relay's
//!   health to `Degraded`;
//! - (c) a startup `run()` **drains a pre-existing `Pending` row** (crash
//!   recovery) without any external wake.
//!
//! The nominal `run_once` flow lives in `outbox_e2e.rs`; concurrency/retry in
//! `outbox_concurrency_e2e.rs`. Shared fixtures + run-gating doc in `outbox_common`.
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

/// (a) NOTIFY-WAKE — staging a row wakes a parked `run()` relay and it publishes,
/// with no polling. The relay is started first (parked on its `select!`), then a
/// row is staged in a committed transaction; the `pg_notify` fired at commit is
/// the only thing that can wake the relay, so the row reaching the stream proves
/// the subscribe path.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn staging_a_row_wakes_the_running_relay_and_publishes() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let prefix = unique_prefix();
    let js = jetstream().await;
    let (publisher, consumer, stream_name) = setup_stream(&js, &prefix).await;

    // Start the relay FIRST — it drains the (empty) outbox and parks on NOTIFY.
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

    // Give the relay a moment to LISTEN + park (so the NOTIFY below is what wakes
    // it — not a startup drain that happened to catch the row).
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Stage a row in a committed transaction — the commit fires the NOTIFY.
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
    tx.commit().await.expect("commit"); // → NOTIFY wakes the parked relay

    // The relay, woken by the NOTIFY, publishes the row.
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

/// (b) STRUCTURAL FAILURE — a publish to a subject **no stream captures** answers
/// `NoStream`. The row must stay `Pending` with its attempt budget untouched, and
/// the relay's health must flip to `Degraded`. We start the relay, stage a row on
/// an undeclared subject, and assert both effects.
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn a_structural_publish_failure_keeps_the_row_pending_and_degrades_health() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    // A real NATS publisher, but NO stream declared for the subject we stage —
    // JetStream answers the publish with `StreamNotFound` (our `NoStream`).
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

    // Stage a row whose subject is captured by NO declared stream → structural.
    let subject = integration_subject(&undeclared_prefix, MessageKind::Evt, "thing", "happened", 1)
        .expect("subject");
    let row_id = Uuid::now_v7();
    let record =
        OutboxRecord::stage_event(row_id, &subject, &sample_event(Uuid::now_v7())).expect("stage");
    stage_into(&pool, &table, &record).await.expect("stage row");

    // Health degrades.
    assert!(
        await_health(&mut health, Duration::from_secs(5), |h| matches!(
            h,
            RelayHealth::Degraded { .. }
        ))
        .await,
        "a structural NoStream failure must degrade relay health"
    );

    // The row is still PENDING and its attempt budget was NOT consumed.
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

/// (c) STARTUP RECOVERY — a `Pending` row that already exists when `run()` starts
/// is drained by the one startup recovery sweep, with no external wake. This is
/// the crash-recovery path through the `run()` entry point (the bare-`run_once`
/// recovery is covered in `outbox_e2e.rs`).
#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL + NATS_URL (real infra)"]
async fn run_drains_a_preexisting_pending_row_on_startup() {
    let table = unique_table();
    let pool = connect_pool(5).await;
    create_outbox_table(&pool, &table).await;

    let prefix = unique_prefix();
    let js = jetstream().await;
    let (publisher, consumer, stream_name) = setup_stream(&js, &prefix).await;

    // Stage a row BEFORE the relay exists — exactly a crash-left-Pending row.
    let thing_id = Uuid::now_v7();
    let subject =
        integration_subject(&prefix, MessageKind::Evt, "thing", "happened", 1).expect("subject");
    let row_id = Uuid::now_v7();
    let record =
        OutboxRecord::stage_event(row_id, &subject, &sample_event(thing_id)).expect("stage event");
    stage_into(&pool, &table, &record).await.expect("stage row");

    let (status, _, _) = read_row(&pool, &table, row_id).await;
    assert_eq!(status, "PENDING", "the row pre-exists, unpublished");

    // Now start the relay: its startup recovery drain publishes the leftover row
    // with no NOTIFY needed.
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
