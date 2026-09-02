use br_test_support::require_nats_url;
use br_util_nats_fabric::{
    Fabric, FabricError, KV_PUBLISHED_LANGUAGE, KvKey, PublishedLanguagePublisher,
    PublishedLanguageReader,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Payload {
    label: String,
}

fn payload(label: &str) -> Payload {
    Payload {
        label: label.to_string(),
    }
}

async fn jetstream() -> async_nats::jetstream::Context {
    let url = require_nats_url();
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn fabric() -> Fabric {
    Fabric::new(jetstream().await)
}

async fn ensure_published_language_bucket(js: &async_nats::jetstream::Context) {
    if js.get_key_value(KV_PUBLISHED_LANGUAGE).await.is_ok() {
        return;
    }
    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_PUBLISHED_LANGUAGE.to_string(),
        ..Default::default()
    })
    .await
    .expect("create bucket");
}

async fn publisher() -> PublishedLanguagePublisher<Payload> {
    let js = jetstream().await;
    ensure_published_language_bucket(&js).await;
    PublishedLanguagePublisher::open(&fabric().await)
        .await
        .expect("open publisher")
}

fn isolated_key(suffix: &str) -> KvKey {
    KvKey::new(format!("plcas/{}/{suffix}", Uuid::now_v7().simple())).unwrap()
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn get_with_revision_returns_none_for_an_absent_key() {
    require_nats_url();
    let publisher = publisher().await;
    let reader = PublishedLanguageReader::<Payload>::open(&fabric().await)
        .await
        .expect("open reader");
    let key = isolated_key("absent");

    assert!(
        publisher
            .get_with_revision(&key)
            .await
            .expect("publisher get")
            .is_none()
    );
    assert!(
        reader
            .get_with_revision(&key)
            .await
            .expect("reader get")
            .is_none()
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn get_with_revision_returns_none_for_a_retracted_key() {
    require_nats_url();
    let publisher = publisher().await;
    let key = isolated_key("retracted");

    publisher.put(&key, &payload("live")).await.expect("put");
    publisher.retract(&key).await.expect("retract");

    assert!(
        publisher
            .get_with_revision(&key)
            .await
            .expect("get")
            .is_none(),
        "a tombstone reads as absent, never as a decodable value"
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn a_matching_revision_wins_and_returns_the_new_revision() {
    require_nats_url();
    let publisher = publisher().await;
    let reader = PublishedLanguageReader::<Payload>::open(&fabric().await)
        .await
        .expect("open reader");
    let key = isolated_key("cas-win");

    publisher.put(&key, &payload("v1")).await.expect("put");
    let (value, revision) = publisher
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("key is live");
    assert_eq!(value, payload("v1"));

    let next = publisher
        .update_if(&key, &payload("v2"), revision)
        .await
        .expect("cas write on the observed revision");
    assert_ne!(next, revision, "a successful write yields a new revision");

    let (observed, observed_revision) = reader
        .get_with_revision(&key)
        .await
        .expect("reader get")
        .expect("key is live");
    assert_eq!(observed, payload("v2"));
    assert_eq!(
        observed_revision, next,
        "the revision returned by update_if is the one a reader observes"
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn a_stale_revision_is_refused_with_revision_conflict() {
    require_nats_url();
    let publisher = publisher().await;
    let key = isolated_key("cas-stale");

    publisher.put(&key, &payload("v1")).await.expect("put");
    let (_, stale) = publisher
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("key is live");
    publisher
        .update_if(&key, &payload("v2"), stale)
        .await
        .expect("first writer wins");

    let err = publisher
        .update_if(&key, &payload("v3"), stale)
        .await
        .expect_err("the second writer must lose");
    match err {
        FabricError::RevisionConflict {
            key: conflicted, ..
        } => {
            assert_eq!(conflicted, key.as_str());
        }
        other => panic!("expected RevisionConflict, got {other:?}"),
    }

    let (value, _) = publisher
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("key is live");
    assert_eq!(value, payload("v2"), "the losing write left no trace");
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn delete_if_refuses_a_stale_revision_and_accepts_the_current_one() {
    require_nats_url();
    let publisher = publisher().await;
    let key = isolated_key("cas-delete");

    publisher.put(&key, &payload("v1")).await.expect("put");
    let (_, stale) = publisher
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("key is live");
    let current = publisher
        .update_if(&key, &payload("v2"), stale)
        .await
        .expect("cas write");

    let err = publisher
        .delete_if(&key, stale)
        .await
        .expect_err("a stale delete must be refused");
    match err {
        FabricError::RevisionConflict {
            key: conflicted, ..
        } => {
            assert_eq!(conflicted, key.as_str());
        }
        other => panic!("expected RevisionConflict, got {other:?}"),
    }
    assert!(
        publisher
            .get_with_revision(&key)
            .await
            .expect("get")
            .is_some(),
        "the refused delete left the key live"
    );

    publisher
        .delete_if(&key, current)
        .await
        .expect("delete on the current revision");
    assert!(
        publisher
            .get_with_revision(&key)
            .await
            .expect("get")
            .is_none()
    );
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn put_stays_last_writer_wins_against_a_stale_holder() {
    require_nats_url();
    let publisher = publisher().await;
    let key = isolated_key("lww");

    publisher.put(&key, &payload("v1")).await.expect("put");
    let (_, stale) = publisher
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("key is live");
    publisher
        .update_if(&key, &payload("v2"), stale)
        .await
        .expect("cas write");

    publisher
        .put(&key, &payload("v3"))
        .await
        .expect("put ignores revisions");
    publisher
        .update(&key, &payload("v4"))
        .await
        .expect("update ignores revisions");

    let (value, _) = publisher
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("key is live");
    assert_eq!(value, payload("v4"));
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn update_if_after_a_retract_conflicts() {
    require_nats_url();
    let publisher = publisher().await;
    let key = isolated_key("cas-retracted");

    publisher.put(&key, &payload("v1")).await.expect("put");
    let (_, live) = publisher
        .get_with_revision(&key)
        .await
        .expect("get")
        .expect("key is live");
    publisher.retract(&key).await.expect("retract");

    let err = publisher
        .update_if(&key, &payload("v2"), live)
        .await
        .expect_err("the retracted key must refuse its last live revision");
    match err {
        FabricError::RevisionConflict {
            key: conflicted, ..
        } => assert_eq!(conflicted, key.as_str()),
        other => panic!("expected RevisionConflict, got {other:?}"),
    }

    assert!(
        publisher
            .get_with_revision(&key)
            .await
            .expect("get")
            .is_none(),
        "re-reading is what tells a retry loop the key is gone, not the conflict itself"
    );
}
