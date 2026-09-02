#![cfg(all(feature = "consumer", feature = "publisher"))]

mod support;

use std::sync::Arc;

use br_util_directory::{DirectoryProjector, DirectoryPublisher};
use br_util_nats_fabric::WatchHealth;
use uuid::Uuid;

use support::{
    RecordingStager, drop_published_user, exclusive_projection, fabric, infra, isolated_pool,
    manifest, staged_keys, user, user_row, wait_for,
};

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn health_is_degraded_while_no_watch_is_in_flight() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let projector = DirectoryProjector::new(fabric, pool);
    assert_eq!(*projector.health().borrow(), WatchHealth::Degraded);

    exclusive.release().await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_watch_in_flight_reports_healthy() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
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
    .expect("the watch reports healthy while it is in flight");

    watching.abort();
    let _ = watching.await;

    exclusive.release().await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_live_retract_stages_an_impact_only_when_it_deletes_a_row() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let user_id = Uuid::now_v7();
    let sentinel_id = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");

    let projector = Arc::new(
        DirectoryProjector::new(fabric.clone(), pool.clone())
            .with_impact_stager(Arc::new(RecordingStager::accepting())),
    );
    let mut health = projector.health();
    let running = projector.clone();
    let watching = tokio::spawn(async move { running.watch().await });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        while *health.borrow_and_update() != WatchHealth::Healthy {
            health.changed().await.expect("health channel stays open");
        }
    })
    .await
    .expect("the watch is in flight");

    publisher
        .publish_user(user_id, &user("ada@example.test"))
        .await
        .expect("publish the user");
    wait_for("the live put to project", async || {
        user_row(&pool, user_id).await.is_some()
    })
    .await;
    assert_eq!(staged_keys(&pool, user_id).await, 1);

    publisher
        .retract_user(user_id)
        .await
        .expect("first retract");
    wait_for("the live retract to delete the row", async || {
        user_row(&pool, user_id).await.is_none()
    })
    .await;
    assert_eq!(
        staged_keys(&pool, user_id).await,
        2,
        "deleting a row is a change and stages its own impact"
    );

    publisher
        .retract_user(user_id)
        .await
        .expect("second retract");
    publisher
        .publish_user(sentinel_id, &user("sentinel@example.test"))
        .await
        .expect("publish the sentinel");
    wait_for(
        "the sentinel to project behind the second retract",
        async || user_row(&pool, sentinel_id).await.is_some(),
    )
    .await;
    assert_eq!(
        staged_keys(&pool, user_id).await,
        2,
        "a retract that deletes nothing stages nothing"
    );

    watching.abort();
    let _ = watching.await;
    drop_published_user(&fabric, sentinel_id).await;
    exclusive.release().await;
}
