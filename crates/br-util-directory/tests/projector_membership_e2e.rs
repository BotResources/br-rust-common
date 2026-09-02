#![cfg(all(feature = "consumer", feature = "publisher"))]

mod support;

use std::sync::Arc;

use br_util_directory::{DirectoryProjector, DirectoryPublisher, GROUP_NAMESPACE, USER_NAMESPACE};
use uuid::Uuid;

use support::{
    RecordingStager, drop_published_group, exclusive_projection, fabric, group, infra,
    isolated_pool, manifest, members, staged_in, user,
};

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_removed_member_is_named_by_its_own_impact_beside_the_group_impact() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let group_id = Uuid::now_v7();
    let kept = Uuid::now_v7();
    let removed = Uuid::now_v7();
    let joined = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_group(group_id, &group("crew", &[kept, removed]))
        .await
        .expect("publish the group");

    let projector = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::accepting()));
    projector.reconcile().await.expect("first reconcile");
    let mut initial = vec![kept, removed];
    initial.sort();
    assert_eq!(members(&pool, group_id).await, initial);
    assert_eq!(staged_in(&pool, GROUP_NAMESPACE, group_id).await, 1);
    assert_eq!(staged_in(&pool, USER_NAMESPACE, removed).await, 0);

    publisher
        .publish_group(group_id, &group("crew", &[kept]))
        .await
        .expect("republish the group without the removed member");
    projector.reconcile().await.expect("second reconcile");

    assert_eq!(
        members(&pool, group_id).await,
        vec![kept],
        "the membership rewrite dropped exactly the removed member"
    );
    assert_eq!(
        staged_in(&pool, GROUP_NAMESPACE, group_id).await,
        2,
        "the group change is staged"
    );
    assert_eq!(
        staged_in(&pool, USER_NAMESPACE, removed).await,
        1,
        "the removed member is named by its own impact, in the transaction that removed it"
    );
    assert_eq!(
        staged_in(&pool, USER_NAMESPACE, kept).await,
        0,
        "an untouched member is not impacted"
    );

    publisher
        .publish_group(group_id, &group("crew", &[kept, joined]))
        .await
        .expect("republish the group with a new member");
    projector.reconcile().await.expect("third reconcile");

    assert_eq!(
        staged_in(&pool, GROUP_NAMESPACE, group_id).await,
        3,
        "the group change is staged"
    );
    assert_eq!(
        staged_in(&pool, USER_NAMESPACE, joined).await,
        0,
        "an added member is recoverable from the current membership: the group impact suffices"
    );

    drop_published_group(&fabric, group_id).await;
    exclusive.release().await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_retracted_user_impacts_every_group_it_still_belongs_to() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let user_id = Uuid::now_v7();
    let first_group = Uuid::now_v7();
    let second_group = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_user(user_id, &user("ada@example.test"))
        .await
        .expect("publish the user");
    publisher
        .publish_group(first_group, &group("crew", &[user_id]))
        .await
        .expect("publish the first group");
    publisher
        .publish_group(second_group, &group("guild", &[user_id]))
        .await
        .expect("publish the second group");

    let projector = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::accepting()));
    projector.reconcile().await.expect("first reconcile");
    assert_eq!(staged_in(&pool, USER_NAMESPACE, user_id).await, 1);
    assert_eq!(staged_in(&pool, GROUP_NAMESPACE, first_group).await, 1);

    publisher.retract_user(user_id).await.expect("retract");
    projector.reconcile().await.expect("second reconcile");

    assert_eq!(
        staged_in(&pool, USER_NAMESPACE, user_id).await,
        2,
        "the retraction stages the user impact"
    );
    assert_eq!(
        staged_in(&pool, GROUP_NAMESPACE, first_group).await,
        2,
        "every group the retracted user belonged to is impacted"
    );
    assert_eq!(staged_in(&pool, GROUP_NAMESPACE, second_group).await, 2);
    assert_eq!(
        members(&pool, first_group).await,
        vec![user_id],
        "memberships stay group-derived: the retraction does not rewrite them"
    );

    drop_published_group(&fabric, first_group).await;
    drop_published_group(&fabric, second_group).await;
    exclusive.release().await;
}

#[tokio::test]
#[ignore = "requires NATS_URL and TEST_DATABASE_URL pointing at a real broker and Postgres"]
async fn a_retracted_group_impacts_the_members_its_cascade_unlinks() {
    let (nats, database) = infra();
    let exclusive = exclusive_projection(&database).await;
    let fabric = fabric(&nats).await;
    let pool = isolated_pool(&database).await;

    let group_id = Uuid::now_v7();
    let member = Uuid::now_v7();
    let publisher = DirectoryPublisher::open(&fabric).await.expect("publisher");
    publisher.write_meta(&manifest()).await.expect("write meta");
    publisher
        .publish_group(group_id, &group("crew", &[member]))
        .await
        .expect("publish the group");

    let projector = DirectoryProjector::new(fabric.clone(), pool.clone())
        .with_impact_stager(Arc::new(RecordingStager::accepting()));
    projector.reconcile().await.expect("first reconcile");
    assert_eq!(staged_in(&pool, USER_NAMESPACE, member).await, 0);

    publisher
        .retract_group(group_id)
        .await
        .expect("retract the group");
    projector.reconcile().await.expect("second reconcile");

    assert!(
        members(&pool, group_id).await.is_empty(),
        "the group delete cascaded its memberships away"
    );
    assert_eq!(staged_in(&pool, GROUP_NAMESPACE, group_id).await, 2);
    assert_eq!(
        staged_in(&pool, USER_NAMESPACE, member).await,
        1,
        "a member the cascade unlinked is named by its own impact"
    );

    exclusive.release().await;
}
