#![cfg(all(feature = "consumer", feature = "publisher"))]

mod support;

use std::sync::Arc;

use br_util_directory::{DirectoryProjector, DirectoryPublisher};
use uuid::Uuid;

use support::{
    RecordingStager, drop_published_group, drop_published_service_account, exclusive_projection,
    fabric, group, infra, isolated_pool, manifest, manifest_with_service_accounts, members,
    service_account, service_account_name, staged_keys,
};

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_group_stages_an_impact_only_when_its_name_or_member_set_changes() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
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
    exclusive.release().await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_service_account_stages_an_impact_only_when_its_name_changes() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let account_id = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher
        .write_meta(&manifest_with_service_accounts())
        .await
        .expect("write meta");
    publisher
        .publish_service_account(account_id, &service_account("relay"))
        .await
        .expect("publish the service account");

    let projector = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::accepting()));
    let progress = projector.progress();

    projector.reconcile().await.expect("first reconcile");
    let after_first = progress.borrow().changes;
    assert_eq!(
        service_account_name(&pool, account_id).await.as_deref(),
        Some("relay")
    );
    assert_eq!(staged_keys(&pool, account_id).await, 1);

    projector.reconcile().await.expect("idempotent reconcile");
    assert_eq!(progress.borrow().changes, after_first, "no progress bump");
    assert_eq!(staged_keys(&pool, account_id).await, 1, "no second impact");

    publisher
        .publish_service_account(account_id, &service_account("relay-renamed"))
        .await
        .expect("republish the service account");
    projector.reconcile().await.expect("third reconcile");

    assert_eq!(progress.borrow().changes, after_first + 1);
    assert_eq!(staged_keys(&pool, account_id).await, 2);
    assert_eq!(
        service_account_name(&pool, account_id).await.as_deref(),
        Some("relay-renamed")
    );

    drop_published_service_account(&fabric, account_id).await;
    publisher.write_meta(&manifest()).await.expect("reset meta");
    exclusive.release().await;
}
