use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_util_nats_fabric::{
    EphemeralAuthChange, EphemeralAuthStore, Fabric, KV_EPHEMERAL_AUTH, KvKey, WatchHealth,
    WatchHealthReceiver,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Payload {
    label: String,
}

type Seen = Arc<Mutex<Vec<EphemeralAuthChange<Payload>>>>;

fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

async fn create_bucket(js: &async_nats::jetstream::Context) {
    js.create_key_value(async_nats::jetstream::kv::Config {
        bucket: KV_EPHEMERAL_AUTH.to_string(),
        history: 8,
        max_age: Duration::from_secs(3600),
        ..Default::default()
    })
    .await
    .expect("create ephemeral-auth bucket");
}

async fn delete_bucket(js: &async_nats::jetstream::Context) {
    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

async fn reset_bucket(js: &async_nats::jetstream::Context) {
    delete_bucket(js).await;
    create_bucket(js).await;
}

async fn open_store() -> EphemeralAuthStore<Payload> {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    let fabric = Fabric::new(async_nats::jetstream::new(client));
    EphemeralAuthStore::<Payload>::open(&fabric)
        .await
        .expect("open ephemeral-auth store")
}

fn key(suffix: &str) -> KvKey {
    KvKey::new(format!("auth/refresh/{}/{suffix}", Uuid::now_v7().simple())).unwrap()
}

async fn reach_health(rx: &mut WatchHealthReceiver, want: WatchHealth, within: Duration) {
    let observed = tokio::time::timeout(within, async {
        loop {
            if *rx.borrow_and_update() == want {
                return;
            }
            rx.changed().await.expect("health channel alive");
        }
    })
    .await;
    assert!(
        observed.is_ok(),
        "health never reached {want:?} within {within:?}"
    );
}

async fn await_delivery(seen: &Seen, key: &KvKey, within: Duration) {
    let deadline = tokio::time::Instant::now() + within;
    while tokio::time::Instant::now() < deadline {
        let hit = seen.lock().unwrap().iter().any(|change| {
            matches!(change, EphemeralAuthChange::Set { key: k, .. } if k.as_str() == key.as_str())
        });
        if hit {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("no Set for {key:?} delivered within {within:?}");
}

fn spawn_run(
    watcher: br_util_nats_fabric::EphemeralAuthWatcher<Payload>,
    seen: Seen,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        watcher
            .run(move |change| seen.lock().unwrap().push(change))
            .await;
    })
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn run_re_arms_the_watch_after_the_bucket_disappears_and_comes_back() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    reset_bucket(&js).await;

    let store = open_store().await;
    let watcher = store.watcher();
    let mut health = watcher.health();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let running = spawn_run(watcher, seen.clone());

    reach_health(&mut health, WatchHealth::Healthy, Duration::from_secs(10)).await;

    let before = key("before");
    store
        .put(
            &before,
            &Payload {
                label: "before".to_string(),
            },
        )
        .await
        .expect("put before the outage");
    await_delivery(&seen, &before, Duration::from_secs(10)).await;

    delete_bucket(&js).await;
    reach_health(&mut health, WatchHealth::Degraded, Duration::from_secs(20)).await;

    create_bucket(&js).await;
    reach_health(&mut health, WatchHealth::Healthy, Duration::from_secs(60)).await;

    let after = key("after");
    open_store()
        .await
        .put(
            &after,
            &Payload {
                label: "after".to_string(),
            },
        )
        .await
        .expect("put after the outage");
    await_delivery(&seen, &after, Duration::from_secs(30)).await;

    assert!(
        !running.is_finished(),
        "run() must never return; it supervises forever"
    );
    running.abort();
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker"]
async fn run_holds_degraded_and_delivers_nothing_while_the_bucket_is_absent() {
    let Some(_) = nats_url() else { return };
    let js = jetstream().await;
    reset_bucket(&js).await;

    let store = open_store().await;
    let watcher = store.watcher();
    let mut health = watcher.health();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));

    delete_bucket(&js).await;
    let running = spawn_run(watcher, seen.clone());

    tokio::time::sleep(Duration::from_secs(3)).await;

    assert_eq!(
        *health.borrow_and_update(),
        WatchHealth::Degraded,
        "an absent bucket must never read Healthy"
    );
    assert!(
        seen.lock().unwrap().is_empty(),
        "an absent bucket delivers no change"
    );
    assert!(!running.is_finished(), "run() must never return");

    create_bucket(&js).await;
    reach_health(&mut health, WatchHealth::Healthy, Duration::from_secs(60)).await;
    running.abort();
}
