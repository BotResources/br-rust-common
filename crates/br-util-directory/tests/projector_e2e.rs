#![cfg(all(feature = "consumer", feature = "publisher"))]

mod support;

use std::sync::Arc;

use br_util_directory::{DirectoryProjector, DirectoryPublisher, USER_NAMESPACE};
use br_util_nats_fabric::WatchHealth;
use uuid::Uuid;

use support::{
    PROJECTION_LOCK, RecordingStager, drop_published_group, drop_published_user, fabric,
    foreign_ref, group, impacts_for, infra, isolated_pool, manifest, members, staged_keys, user,
    user_row,
};

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn an_unchanged_user_is_a_no_op_write_with_no_impact_and_no_progress() {
    let Some((nats, database)) = infra() else {
        return;
    };
    let _guard = PROJECTION_LOCK.lock().await;
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
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_changed_user_stages_one_impact_in_the_same_transaction_and_bumps_progress() {
    let Some((nats, database)) = infra() else {
        return;
    };
    let _guard = PROJECTION_LOCK.lock().await;
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

    publisher
        .publish_user(user_id, &user("ada.renamed@example.test"))
        .await
        .expect("republish the user");
    projector.reconcile().await.expect("second reconcile");

    assert_eq!(progress.borrow().changes, after_first + 1);
    assert_eq!(staged_keys(&pool, user_id).await, 2);
    assert_eq!(
        user_row(&pool, user_id).await.expect("row").0,
        "ada.renamed@example.test"
    );
    let recorded = seen.lock().expect("record lock").clone();
    let foreign = foreign_ref(recorded.last().expect("an impact"));
    assert_eq!(foreign.namespace(), USER_NAMESPACE);
    assert_eq!(foreign.key(), user_id.to_string());

    drop_published_user(&fabric, user_id).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_failing_stager_rolls_the_roster_upsert_back() {
    let Some((nats, database)) = infra() else {
        return;
    };
    let _guard = PROJECTION_LOCK.lock().await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let user_id = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_user(user_id, &user("ada@example.test"))
        .await
        .expect("publish the user");

    let accepted = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::accepting()));
    accepted.reconcile().await.expect("first reconcile");
    let before = user_row(&pool, user_id).await.expect("the projected row");

    publisher
        .publish_user(user_id, &user("ada.renamed@example.test"))
        .await
        .expect("republish the user");

    let refused = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::failing()));
    let progress = refused.progress();
    refused
        .reconcile()
        .await
        .expect_err("the stager failure surfaces");

    assert_eq!(user_row(&pool, user_id).await.expect("row"), before);
    assert_eq!(
        staged_keys(&pool, user_id).await,
        1,
        "the staged row rolled back too"
    );
    assert_eq!(
        progress.borrow().changes,
        0,
        "a rolled-back write is no progress"
    );

    drop_published_user(&fabric, user_id).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_group_stages_an_impact_only_when_its_name_or_member_set_changes() {
    let Some((nats, database)) = infra() else {
        return;
    };
    let _guard = PROJECTION_LOCK.lock().await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let group_id = Uuid::now_v7();
    let first_member = Uuid::now_v7();
    let second_member = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_group(group_id, &group("crew", &[first_member]))
        .await
        .expect("publish the group");

    let projector = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::accepting()));
    let progress = projector.progress();

    projector.reconcile().await.expect("first reconcile");
    let after_first = progress.borrow().changes;
    assert_eq!(staged_keys(&pool, group_id).await, 1);
    assert_eq!(members(&pool, group_id).await, vec![first_member]);

    projector.reconcile().await.expect("idempotent reconcile");
    assert_eq!(progress.borrow().changes, after_first, "no progress bump");
    assert_eq!(
        staged_keys(&pool, group_id).await,
        1,
        "same name and same member set is not a change"
    );

    publisher
        .publish_group(group_id, &group("crew", &[first_member, second_member]))
        .await
        .expect("republish the group");
    projector.reconcile().await.expect("third reconcile");

    assert_eq!(progress.borrow().changes, after_first + 1);
    assert_eq!(staged_keys(&pool, group_id).await, 2);
    let mut expected = vec![first_member, second_member];
    expected.sort();
    assert_eq!(members(&pool, group_id).await, expected);

    drop_published_group(&fabric, group_id).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn without_a_stager_the_roster_projects_and_converges_as_before() {
    let Some((nats, database)) = infra() else {
        return;
    };
    let _guard = PROJECTION_LOCK.lock().await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let user_id = Uuid::now_v7();
    let group_id = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_user(user_id, &user("ada@example.test"))
        .await
        .expect("publish the user");
    publisher
        .publish_group(group_id, &group("crew", &[user_id]))
        .await
        .expect("publish the group");

    let projector = DirectoryProjector::new(fabric.clone(), pool.clone());
    projector.reconcile().await.expect("first reconcile");
    assert_eq!(
        user_row(&pool, user_id).await.expect("row").0,
        "ada@example.test"
    );
    assert_eq!(members(&pool, group_id).await, vec![user_id]);
    assert_eq!(staged_keys(&pool, user_id).await, 0);

    publisher
        .publish_user(user_id, &user("ada.renamed@example.test"))
        .await
        .expect("republish the user");
    projector.reconcile().await.expect("second reconcile");
    assert_eq!(
        user_row(&pool, user_id).await.expect("row").0,
        "ada.renamed@example.test"
    );

    publisher.retract_user(user_id).await.expect("retract");
    projector.reconcile().await.expect("third reconcile");
    assert!(user_row(&pool, user_id).await.is_none(), "orphan-deleted");

    drop_published_group(&fabric, group_id).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn health_is_degraded_while_no_watch_is_running() {
    let Some((nats, database)) = infra() else {
        return;
    };
    let _guard = PROJECTION_LOCK.lock().await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let projector = DirectoryProjector::new(fabric, pool);
    assert_eq!(*projector.health().borrow(), WatchHealth::Degraded);
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_running_watch_reports_healthy() {
    let Some((nats, database)) = infra() else {
        return;
    };
    let _guard = PROJECTION_LOCK.lock().await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");

    let projector = Arc::new(DirectoryProjector::new(fabric, pool));
    let mut health = projector.health();
    let running = projector.clone();
    let watching = tokio::spawn(async move { running.watch().await });

    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while *health.borrow_and_update() != WatchHealth::Healthy {
            health.changed().await.expect("health channel stays open");
        }
    })
    .await
    .expect("the watch reports healthy");

    watching.abort();
    let _ = watching.await;
}
