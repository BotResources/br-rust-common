mod outbox_fixture;

use br_util_nats_fabric::{Fabric, INTEGRATION_EVT, OutboxRecord, OutboxRelay, stage};
use outbox_fixture::{
    DUPLICATE_WINDOW, coords, delivered_for, envelope, jetstream, message_id, outbox_pool,
    recreate_event_stream, rewind_to_pending, row_status, user_created,
};
use std::time::Duration;
use uuid::Uuid;

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

    let stored = delivered_for(&js, &[event_id]).await;
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

    let stored = delivered_for(&js, &[event_id]).await;
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

    let stored = delivered_for(&js, &[row_id]).await;
    assert_eq!(stored.len(), 1);
    assert_eq!(message_id(&stored[0]), row_id.to_string());

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and database"]
async fn one_event_id_reused_across_two_coordinates_loses_the_second_frame_stream_wide() {
    let js = jetstream().await;
    recreate_event_stream(&js).await;
    let pool = outbox_pool().await;
    let relay = OutboxRelay::new(pool.clone(), Fabric::new(jetstream().await));

    let event_id = Uuid::now_v7();
    for destination in [user_created(), coords("activated")] {
        let record = OutboxRecord::stage_event(Uuid::now_v7(), destination, &envelope(event_id))
            .expect("stage");
        stage(&pool, &record).await.expect("persist the staged row");
    }

    let pass = relay.run_once_detailed().await.expect("relay pass");
    assert_eq!(pass.picked, 2);
    assert_eq!(
        pass.published, 2,
        "both rows are accepted and marked published"
    );
    assert_eq!(
        pass.duplicates, 1,
        "dedup is stream-wide, so the second coordinate collides on the same event_id"
    );

    let stored = delivered_for(&js, &[event_id]).await;
    assert_eq!(
        stored.len(),
        1,
        "the frame for the second coordinate is dropped by the broker, not delivered"
    );
    assert_eq!(
        stored[0].subject.as_str(),
        "integration.evt.identity.user.created.v1",
        "only the first coordinate survives"
    );

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and database"]
async fn two_raw_staged_rows_sharing_a_uuid_event_id_collapse_to_one_frame() {
    let js = jetstream().await;
    recreate_event_stream(&js).await;
    let pool = outbox_pool().await;
    let relay = OutboxRelay::new(pool.clone(), Fabric::new(jetstream().await));

    let event_id = Uuid::now_v7();
    let first_row = Uuid::now_v7();
    let second_row = Uuid::now_v7();
    for row_id in [first_row, second_row] {
        let record = OutboxRecord::stage(
            row_id,
            user_created(),
            serde_json::json!({
                "event_id": event_id.to_string(),
                "hand_rolled": "no envelope type, just a top-level event_id",
            }),
        );
        stage(&pool, &record).await.expect("persist the staged row");
    }

    let pass = relay.run_once_detailed().await.expect("relay pass");
    assert_eq!(pass.picked, 2);
    assert_eq!(
        pass.published, 2,
        "both rows are accepted and marked published"
    );
    assert_eq!(
        pass.row_id_fallbacks, 0,
        "a raw-staged payload whose top-level event_id is a UUID is promoted to the dedup key"
    );
    assert_eq!(
        pass.duplicates, 1,
        "the second raw row collides on the event_id its caller chose"
    );

    assert_eq!(row_status(&pool, first_row).await, "PUBLISHED");
    assert_eq!(
        row_status(&pool, second_row).await,
        "PUBLISHED",
        "the dropped frame still leaves its row PUBLISHED — the caller owns the uniqueness of event_id"
    );

    let stored = delivered_for(&js, &[event_id]).await;
    assert_eq!(
        stored.len(),
        1,
        "the broker drops the second frame inside the duplicate window"
    );
    assert_eq!(message_id(&stored[0]), event_id.to_string());

    let _ = js.delete_stream(INTEGRATION_EVT).await;
}
