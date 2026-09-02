mod watch_liveness;

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use br_test_support::require_nats_url;
use br_util_nats_fabric::{
    EphemeralAuthChange, EphemeralAuthStore, Fabric, KV_EPHEMERAL_AUTH, KvKey, WatchHealth,
    WatchHealthReceiver, WatchProgress, WatchProgressReceiver,
};
use serde::{Deserialize, Serialize};
use tracing_subscriber::fmt::MakeWriter;
use uuid::Uuid;
use watch_liveness::live_watch_baseline;

const SENTINEL: &str = "SENTINEL-SECRET-9f13c7a2";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Session {
    label: String,
    generation: u64,
}

type Seen = Arc<Mutex<Vec<EphemeralAuthChange<Session>>>>;

#[derive(Clone, Default)]
struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

impl CapturedLogs {
    fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }
}

impl Write for CapturedLogs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for CapturedLogs {
    type Writer = Self;

    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

fn capture_warnings() -> CapturedLogs {
    static INSTALLED: OnceLock<CapturedLogs> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            let logs = CapturedLogs::default();
            let subscriber = tracing_subscriber::fmt()
                .with_writer(logs.clone())
                .with_max_level(tracing::Level::WARN)
                .with_ansi(false)
                .finish();
            tracing::subscriber::set_global_default(subscriber)
                .expect("this suite installs the only global subscriber in its binary");
            logs
        })
        .clone()
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
        history: 8,
        max_age: Duration::from_secs(3600),
        ..Default::default()
    })
    .await
    .expect("create ephemeral-auth bucket");
}

async fn raw_bucket(js: &async_nats::jetstream::Context) -> async_nats::jetstream::kv::Store {
    js.get_key_value(KV_EPHEMERAL_AUTH)
        .await
        .expect("the bucket the fixture just created")
}

async fn open_store() -> EphemeralAuthStore<Session> {
    let client = async_nats::connect(&require_nats_url())
        .await
        .expect("connect to NATS");
    let fabric = Fabric::new(async_nats::jetstream::new(client));
    EphemeralAuthStore::<Session>::open(&fabric)
        .await
        .expect("open ephemeral-auth store")
}

fn spawn_run(
    watcher: br_util_nats_fabric::EphemeralAuthWatcher<Session>,
    seen: Seen,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        watcher
            .run(move |change| seen.lock().unwrap().push(change))
            .await;
    })
}

async fn reach_healthy(rx: &mut WatchHealthReceiver, within: Duration) {
    let reached = tokio::time::timeout(within, async {
        loop {
            if *rx.borrow_and_update() == WatchHealth::Healthy {
                return;
            }
            rx.changed().await.expect("health channel alive");
        }
    })
    .await;
    assert!(reached.is_ok(), "the watch never reached Healthy");
}

async fn await_progress(
    rx: &mut WatchProgressReceiver,
    want: impl Fn(WatchProgress) -> bool,
    within: Duration,
) -> WatchProgress {
    tokio::time::timeout(within, async {
        loop {
            let progress = *rx.borrow_and_update();
            if want(progress) {
                return progress;
            }
            rx.changed().await.expect("progress channel alive");
        }
    })
    .await
    .expect("the watch never reached the expected progress")
}

fn touches(change: &EphemeralAuthChange<Session>, raw_key: &str) -> bool {
    match change {
        EphemeralAuthChange::Set { key, .. } | EphemeralAuthChange::Removed { key } => {
            key.as_str() == raw_key
        }
    }
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker; recreates the shared EPHEMERAL_AUTH bucket, so run with --test-threads=1"]
async fn a_type_mismatched_value_is_skipped_without_its_bytes_reaching_a_log_line() {
    let poison_value = format!(r#"{{"label":"rotated","generation":"{SENTINEL}"}}"#);
    let leak = serde_json::from_str::<Session>(&poison_value)
        .expect_err("the fixture value must not decode")
        .to_string();
    assert!(
        leak.contains(SENTINEL),
        "the serde message must quote the offending value, or this test proves nothing: {leak}"
    );

    let logs = capture_warnings();
    let js = jetstream().await;
    reset_bucket(&js).await;
    let raw = raw_bucket(&js).await;

    let store = open_store().await;
    let watcher = store.watcher();
    let mut health = watcher.health();
    let mut progress = watcher.progress();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let running = spawn_run(watcher, seen.clone());
    reach_healthy(&mut health, Duration::from_secs(10)).await;
    let baseline = live_watch_baseline(&js, &mut progress).await;

    let poison_key = format!("auth/refresh/{}/poison", Uuid::now_v7().simple());
    raw.put(&poison_key, poison_value.clone().into())
        .await
        .expect("a schema-skewed writer puts a credential-bearing value of the wrong shape");
    await_progress(
        &mut progress,
        |p| p.skipped == baseline.skipped + 1,
        Duration::from_secs(10),
    )
    .await;

    let captured = logs.text();
    assert!(
        captured.contains(&poison_key),
        "the warn line naming the skipped key was never captured: {captured}"
    );
    assert!(
        captured.contains(r#"reason="undecodable value""#),
        "the warn line must carry the static reason discriminant: {captured}"
    );
    assert!(
        captured.contains(&format!("value_len={}", poison_value.len())),
        "the warn line must carry the value byte length: {captured}"
    );
    assert!(
        !captured.contains(SENTINEL),
        "the offending value leaked into a log line: {captured}"
    );
    assert!(
        !seen.lock().unwrap().iter().any(|c| touches(c, &poison_key)),
        "an undecodable value is never handed to the caller"
    );

    running.abort();
    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}

#[tokio::test]
#[ignore = "requires NATS_URL pointing at a JetStream-enabled broker; recreates the shared EPHEMERAL_AUTH bucket, so run with --test-threads=1"]
async fn a_key_this_crate_rejects_is_skipped_for_both_its_put_and_its_delete() {
    let js = jetstream().await;
    reset_bucket(&js).await;
    let raw = raw_bucket(&js).await;

    let store = open_store().await;
    let watcher = store.watcher();
    let mut health = watcher.health();
    let mut progress = watcher.progress();
    let seen: Seen = Arc::new(Mutex::new(Vec::new()));
    let running = spawn_run(watcher, seen.clone());
    reach_healthy(&mut health, Duration::from_secs(10)).await;
    let baseline = live_watch_baseline(&js, &mut progress).await;

    let alien_key = format!("auth/refresh/{}=rotated", Uuid::now_v7().simple());
    assert!(
        KvKey::new(alien_key.clone()).is_err(),
        "NATS accepts '=' in a key and this crate does not — that gap is the case under test"
    );
    let readable = serde_json::to_vec(&Session {
        label: "alien".to_string(),
        generation: 1,
    })
    .unwrap();
    raw.put(&alien_key, readable.into())
        .await
        .expect("NATS accepts the key this crate rejects");
    raw.delete(&alien_key)
        .await
        .expect("delete the same alien key");

    let observed = await_progress(
        &mut progress,
        |p| p.skipped == baseline.skipped + 2,
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(
        observed.changes, baseline.changes,
        "neither the put nor the delete of an unusable key is delivered"
    );
    assert!(
        !seen.lock().unwrap().iter().any(|c| touches(c, &alien_key)),
        "a Removed for a key whose Set was skipped is skipped too, so no cache can hold it"
    );
    assert_eq!(
        *health.borrow_and_update(),
        WatchHealth::Healthy,
        "an unusable key does not degrade a live watch"
    );
    assert!(!running.is_finished(), "the watch keeps running");

    running.abort();
    let _ = js.delete_key_value(KV_EPHEMERAL_AUTH).await;
}
