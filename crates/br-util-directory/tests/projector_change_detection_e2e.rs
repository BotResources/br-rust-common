#![cfg(all(feature = "consumer", feature = "publisher"))]

mod support;

use std::sync::Arc;

use br_util_directory::{DirectoryProjector, DirectoryPublisher};
use uuid::Uuid;

use support::{
    RecordingStager, drop_published_user, exclusive_projection, fabric, impacts_for, infra,
    isolated_pool, manifest, nameless_user, staged_keys, user, user_row,
};

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn an_unchanged_user_is_a_no_op_write_with_no_impact_and_no_progress() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let user_id = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_user(user_id, &user("ada@example.test"))
        .await
        .expect("publish the user");

    let stager = Arc::new(RecordingStager::accepting());
    let seen = stager.seen();
    let projector =
        DirectoryProjector::new(fabric.clone(), pool.clone()).with_impact_stager(stager.clone());
    let progress = projector.progress();

    projector.reconcile().await.expect("first reconcile");
    let after_first = progress.borrow().changes;
    let first_row = user_row(&pool, user_id)
        .await
        .expect("the user is projected");
    assert_eq!(staged_keys(&pool, user_id).await, 1);

    projector.reconcile().await.expect("second reconcile");

    assert_eq!(progress.borrow().changes, after_first, "no progress bump");
    assert_eq!(staged_keys(&pool, user_id).await, 1, "no second impact");
    assert_eq!(
        user_row(&pool, user_id).await.expect("row").1,
        first_row.1,
        "xmin is unchanged: the upsert wrote no new row version"
    );
    assert_eq!(
        impacts_for(&seen, user_id),
        1,
        "the stager saw exactly one impact for this user"
    );

    drop_published_user(&fabric, user_id).await;
    exclusive.release().await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_null_name_is_change_detected_like_any_other_value() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let user_id = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_user(user_id, &nameless_user("ada@example.test"))
        .await
        .expect("publish the nameless user");

    let projector = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::accepting()));
    projector.reconcile().await.expect("first reconcile");
    let nameless = user_row(&pool, user_id)
        .await
        .expect("the nameless user is projected");
    assert_eq!(staged_keys(&pool, user_id).await, 1);

    publisher
        .publish_user(user_id, &nameless_user("ada@example.test"))
        .await
        .expect("republish the same nameless user");
    projector.reconcile().await.expect("second reconcile");

    assert_eq!(
        user_row(&pool, user_id).await.expect("row").1,
        nameless.1,
        "NULL is not distinct from NULL: the upsert wrote no new row version"
    );
    assert_eq!(staged_keys(&pool, user_id).await, 1, "no second impact");

    publisher
        .publish_user(user_id, &user("ada@example.test"))
        .await
        .expect("republish the user with a name");
    projector.reconcile().await.expect("third reconcile");

    assert_ne!(
        user_row(&pool, user_id).await.expect("row").1,
        nameless.1,
        "NULL to a value is a change the guard detects"
    );
    assert_eq!(
        staged_keys(&pool, user_id).await,
        2,
        "the named user stages its own impact"
    );

    drop_published_user(&fabric, user_id).await;
    exclusive.release().await;
}
