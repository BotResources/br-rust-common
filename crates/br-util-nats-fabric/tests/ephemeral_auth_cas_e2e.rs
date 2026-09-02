use std::time::Duration;

use br_test_support::require_nats_url;
use br_util_nats_fabric::{
    EphemeralAuthStore, Fabric, FabricError, KV_EPHEMERAL_AUTH, KvKey, Revision,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Family {
    generation: u32,
}

async fn jetstream() -> async_nats::jetstream::Context {
    let client = async_nats::connect(&require_nats_url())
        .await
        .expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn reset_bucket(js: &async_nats::jetstream::Context) {
    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_EPHEMERAL_AUTH.to_string(),
        history: 16,
        max_age: Duration::from_secs(3600),
        ..Default::default()
    })
    .await
    .expect("the fixture, never the lib, declares the bucket");
}

async fn open_store() -> EphemeralAuthStore<Family> {
    let client = async_nats::connect(&require_nats_url())
        .await
        .expect("connect to NATS");
    EphemeralAuthStore::<Family>::open(&Fabric::new(async_nats::jetstream::new(client)))
        .await
        .expect("open ephemeral-auth store")
}

fn key() -> KvKey {
    KvKey::new(format!("auth/refresh/{}", Uuid::now_v7().simple())).unwrap()
}

async fn seeded_family(store: &EphemeralAuthStore<Family>) -> (KvKey, Revision) {
    let k = key();
    store
        .create(&k, &Family { generation: 1 })
        .await
        .expect("create the family");
    let (_, revision) = store
        .get_with_revision(&k)
        .await
        .expect("read the family")
        .expect("the family is live");
    (k, revision)
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker; recreates the shared EPHEMERAL_AUTH bucket, so run with --test-threads=1"]
async fn a_rotation_chains_into_a_revision_checked_delete_without_a_re_read() {
    let js = jetstream().await;
    reset_bucket(&js).await;
    let store = open_store().await;
    let (k, created) = seeded_family(&store).await;

    let rotated = store
        .update_if_returning_revision(&k, &Family { generation: 2 }, created)
        .await
        .expect("rotate against the observed revision");

    assert_ne!(
        rotated, created,
        "a successful rotation moves the revision on"
    );

    store
        .delete_if(&k, rotated)
        .await
        .expect("the revision returned by the rotation closes the chain with no intervening read");

    assert!(
        store
            .get_with_revision(&k)
            .await
            .expect("read after the chained delete")
            .is_none(),
        "the chained delete tombstoned the family"
    );

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker; recreates the shared EPHEMERAL_AUTH bucket, so run with --test-threads=1"]
async fn the_revision_observed_before_a_rotation_no_longer_closes_the_chain() {
    let js = jetstream().await;
    reset_bucket(&js).await;
    let store = open_store().await;
    let (k, created) = seeded_family(&store).await;

    store
        .update_if_returning_revision(&k, &Family { generation: 2 }, created)
        .await
        .expect("rotate against the observed revision");

    let err = store
        .delete_if(&k, created)
        .await
        .expect_err("the pre-rotation revision is stale");
    match err {
        FabricError::RevisionConflict { key, .. } => assert_eq!(key, k.as_str()),
        other => panic!("expected RevisionConflict, got {other:?}"),
    }

    let (still_there, _) = store
        .get_with_revision(&k)
        .await
        .expect("read after the refused delete")
        .expect("a refused revision-checked delete leaves the family live");
    assert_eq!(still_there, Family { generation: 2 });

    let err = store
        .update_if_returning_revision(&k, &Family { generation: 3 }, created)
        .await
        .expect_err("the pre-rotation revision is stale for a write too");
    assert!(matches!(err, FabricError::RevisionConflict { .. }));

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker; recreates the shared EPHEMERAL_AUTH bucket, so run with --test-threads=1"]
async fn the_unit_returning_rotation_still_writes_and_still_needs_a_re_read() {
    let js = jetstream().await;
    reset_bucket(&js).await;
    let store = open_store().await;
    let (k, created) = seeded_family(&store).await;

    store
        .update_if(&k, &Family { generation: 2 }, created)
        .await
        .expect("the unit-returning rotation writes");

    let (value, current) = store
        .get_with_revision(&k)
        .await
        .expect("re-read")
        .expect("the family is live");
    assert_eq!(value, Family { generation: 2 });
    assert_ne!(current, created);

    store
        .delete_if(&k, current)
        .await
        .expect("the re-read revision closes the chain");

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}
