use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_util_nats_fabric::{
    EphemeralAuthChange, EphemeralAuthStore, Fabric, FabricError, KV_EPHEMERAL_AUTH, KvKey,
    KvPrefix,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Payload {
    label: String,
}

fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

async fn fabric() -> Fabric {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    Fabric::new(async_nats::jetstream::new(client))
}

async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn ensure_ttl_bucket(
    js: &async_nats::jetstream::Context,
) -> async_nats::jetstream::kv::Store {
    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_EPHEMERAL_AUTH.to_string(),
        history: 8,
        max_age: Duration::from_secs(3600),
        limit_markers: Some(Duration::from_secs(1)),
        ..Default::default()
    })
    .await
    .expect("create ttl-enabled ephemeral-auth bucket")
}

fn key(prefix: &str, suffix: &str) -> KvKey {
    KvKey::new(format!("{prefix}/{}/{suffix}", Uuid::now_v7().simple())).unwrap()
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn create_with_ttl_expires_a_key_before_the_bucket_max_age() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_ttl_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let k = key("auth/code", "ttl");

    store
        .create_with_ttl(
            &k,
            &Payload {
                label: "one-time".to_string(),
            },
            Duration::from_secs(2),
        )
        .await
        .expect("create with ttl");

    assert!(
        store.get_with_revision(&k).await.expect("get").is_some(),
        "key is live immediately after the ttl write"
    );

    tokio::time::sleep(Duration::from_secs(5)).await;

    assert!(
        store.get_with_revision(&k).await.expect("get").is_none(),
        "key has expired well before the 3600s bucket max_age"
    );

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn keys_and_entries_enumerate_live_keys_and_exclude_tombstones() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_ttl_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let scope = Uuid::now_v7().simple().to_string();
    let prefix = KvPrefix::new(format!("auth/enum/{scope}/")).unwrap();

    let live = KvKey::new(format!("auth/enum/{scope}/live")).unwrap();
    let gone = KvKey::new(format!("auth/enum/{scope}/gone")).unwrap();
    store
        .create(
            &live,
            &Payload {
                label: "live".to_string(),
            },
        )
        .await
        .expect("create live");
    store
        .create(
            &gone,
            &Payload {
                label: "gone".to_string(),
            },
        )
        .await
        .expect("create gone");
    store.delete(&gone).await.expect("delete gone");

    let keys = store.keys(&prefix).await.expect("keys");
    assert_eq!(
        keys,
        vec![live.clone()],
        "tombstoned key excluded from keys"
    );

    let entries = store.entries(&prefix).await.expect("entries");
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries.get(&live),
        Some(&Payload {
            label: "live".to_string()
        })
    );
    assert!(
        !entries.contains_key(&gone),
        "tombstoned key excluded from entries"
    );

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn entries_fail_closed_on_a_malformed_value() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    let raw = ensure_ttl_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let scope = Uuid::now_v7().simple().to_string();
    let prefix = KvPrefix::new(format!("auth/bad/{scope}/")).unwrap();
    let garbage = format!("auth/bad/{scope}/garbage");

    raw.put(&garbage, "{ not json".into())
        .await
        .expect("seed garbage");

    let err = store.entries(&prefix).await.expect_err("must fail closed");
    match err {
        FabricError::Decode { subject, .. } => assert_eq!(subject, garbage),
        other => panic!("expected Decode, got {other:?}"),
    }

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn watch_yields_removed_on_ttl_expiry() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_ttl_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let watcher = store.watcher();
    let seen: Arc<Mutex<Vec<EphemeralAuthChange<Payload>>>> = Arc::new(Mutex::new(Vec::new()));

    let sink = seen.clone();
    let handle = tokio::spawn(async move {
        let _ = watcher
            .watch(move |change| sink.lock().unwrap().push(change))
            .await;
    });

    tokio::time::sleep(Duration::from_millis(300)).await;

    let k = key("auth/expiry", "fam");
    store
        .create_with_ttl(
            &k,
            &Payload {
                label: "short-lived".to_string(),
            },
            Duration::from_secs(2),
        )
        .await
        .expect("create with ttl");

    tokio::time::sleep(Duration::from_secs(6)).await;
    handle.abort();

    let changes = seen.lock().unwrap().clone();
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, EphemeralAuthChange::Removed { key } if key == &k)),
        "a Removed change is emitted when the per-key TTL expires: {changes:?}"
    );

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn watch_yields_set_and_removed_change_events() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    ensure_ttl_bucket(&js).await;
    let fabric = fabric().await;

    let store = EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open store");
    let watcher = store.watcher();
    let seen: Arc<Mutex<Vec<EphemeralAuthChange<Payload>>>> = Arc::new(Mutex::new(Vec::new()));

    let sink = seen.clone();
    let handle = tokio::spawn(async move {
        let _ = watcher
            .watch(move |change| sink.lock().unwrap().push(change))
            .await;
    });

    tokio::time::sleep(Duration::from_millis(300)).await;

    let k = key("auth/watch", "fam");
    store
        .create(
            &k,
            &Payload {
                label: "rotated".to_string(),
            },
        )
        .await
        .expect("create");
    store.delete(&k).await.expect("delete");

    tokio::time::sleep(Duration::from_millis(500)).await;
    handle.abort();

    let changes = seen.lock().unwrap().clone();
    assert!(
        changes.iter().any(|c| matches!(
            c,
            EphemeralAuthChange::Set { key, value }
                if key == &k && value.label == "rotated"
        )),
        "a Set change for the put is observed: {changes:?}"
    );
    assert!(
        changes
            .iter()
            .any(|c| matches!(c, EphemeralAuthChange::Removed { key } if key == &k)),
        "a Removed change for the delete is observed: {changes:?}"
    );

    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}
