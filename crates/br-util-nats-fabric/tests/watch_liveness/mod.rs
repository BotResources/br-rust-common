use std::time::Duration;

use br_util_nats_fabric::{KV_EPHEMERAL_AUTH, WatchProgress, WatchProgressReceiver};
use uuid::Uuid;

const PROBE_INTERVAL: Duration = Duration::from_millis(250);
const LIVENESS_DEADLINE: Duration = Duration::from_secs(30);
const QUIESCENCE: Duration = Duration::from_millis(500);

pub async fn live_watch_baseline(
    js: &async_nats::jetstream::Context,
    rx: &mut WatchProgressReceiver,
) -> WatchProgress {
    let raw = js
        .get_key_value(KV_EPHEMERAL_AUTH)
        .await
        .expect("the bucket the fixture created");
    let deadline = tokio::time::Instant::now() + LIVENESS_DEADLINE;
    loop {
        rx.borrow_and_update();
        let probe = format!("auth/liveness/{}/probe", Uuid::now_v7().simple());
        raw.delete(&probe)
            .await
            .expect("write a liveness marker the watch must observe");
        if tokio::time::timeout(PROBE_INTERVAL, rx.changed())
            .await
            .is_ok()
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "the watch never observed a liveness marker within {LIVENESS_DEADLINE:?}"
        );
    }
    while tokio::time::timeout(QUIESCENCE, rx.changed()).await.is_ok() {}
    *rx.borrow_and_update()
}
