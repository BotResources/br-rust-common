//! Shared helpers for the JetStream e2e suites (durable consumer + awaiter).
//!
//! Same gating as `tests/nats.rs`: `#[ignore]` by default, opted into via
//! `cargo test -- --ignored`, and requiring `NATS_URL` to point at a
//! JetStream-enabled broker. Each test uses a unique stream/subject prefix so
//! the suites can run without colliding; teardown deletes the stream.
//!
//! Each e2e test binary includes this module and uses only a subset of the
//! helpers, so `dead_code` is expected and silenced here (the standard shared
//! test-helper pattern).
#![allow(dead_code)]

use br_core_integration::{IntegrationCommand, IntegrationEvent, MessageMetadata};
use br_core_kernel::{Actor, UserId};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A minimal command/event payload used across the consumer/awaiter e2e tests.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TestPayload {
    pub label: String,
}

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

/// Unique per-test stream/subject prefix (full v7 uuid — see `tests/nats.rs`
/// for why truncation collided in practice).
pub fn unique_prefix() -> String {
    format!("br_test_{}", Uuid::now_v7().simple())
}

pub fn metadata(correlation_id: Uuid) -> MessageMetadata {
    MessageMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id)
}

pub fn command(label: &str, correlation_id: Uuid) -> IntegrationCommand<TestPayload> {
    IntegrationCommand::new(
        Uuid::now_v7(),
        "service_scope.declare",
        1,
        Utc::now(),
        metadata(correlation_id),
        TestPayload {
            label: label.to_string(),
        },
    )
}

pub fn event(event_type: &str, label: &str, correlation_id: Uuid) -> IntegrationEvent<TestPayload> {
    IntegrationEvent::new(
        Uuid::now_v7(),
        event_type,
        1,
        Utc::now(),
        metadata(correlation_id),
        TestPayload {
            label: label.to_string(),
        },
    )
}

/// Connect and return a JetStream context.
pub async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

/// Create a stream capturing `{prefix}.>`, named `STREAM_{prefix}`. Starts from
/// a clean slate (deletes any leftover from a failed run).
pub async fn create_stream(
    js: &async_nats::jetstream::Context,
    prefix: &str,
) -> async_nats::jetstream::stream::Stream {
    let name = format!("STREAM_{prefix}");
    let _ = js.delete_stream(&name).await;
    js.create_stream(async_nats::jetstream::stream::Config {
        name,
        subjects: vec![format!("{prefix}.>")],
        ..Default::default()
    })
    .await
    .expect("create stream")
}

/// Create a durable pull consumer named `durable` on the stream, filtered to
/// `filter_subject`, with explicit ack so nak/term redelivery semantics apply.
pub async fn create_durable(
    stream: &async_nats::jetstream::stream::Stream,
    durable: &str,
    filter_subject: &str,
) {
    stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some(durable.to_string()),
            filter_subject: filter_subject.to_string(),
            ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
            // Short ack-wait so a nak is redelivered fast enough for a test.
            ack_wait: std::time::Duration::from_secs(2),
            ..Default::default()
        })
        .await
        .expect("create durable consumer");
}

/// Delete the stream. Best-effort but loud (a leaked stream captures a later
/// test's messages).
pub async fn teardown(js: &async_nats::jetstream::Context, prefix: &str) {
    let name = format!("STREAM_{prefix}");
    if let Err(e) = js.delete_stream(&name).await {
        eprintln!("teardown: failed to delete stream {name}: {e}");
    }
}

/// This process's cumulative CPU time (user + system) in seconds, sampled via
/// `ps` (portable on macOS and Linux CI). Used by the zero-CPU-idle e2e to
/// prove the durable consumer parks rather than polls. `ps -o time=` prints
/// `[[DD-]HH:]MM:SS`.
///
/// Returns `None` if `ps` is unavailable (e.g. a minimal CI image) so the caller
/// can skip the CPU assertion cleanly instead of failing on the environment.
pub fn process_cpu_seconds() -> Option<f64> {
    let pid = std::process::id();
    let out = match std::process::Command::new("ps")
        .args(["-o", "time=", "-p", &pid.to_string()])
        .output()
    {
        Ok(out) => out,
        Err(e) => {
            eprintln!("process_cpu_seconds: `ps` unavailable ({e}); skipping CPU sample");
            return None;
        }
    };
    let raw = String::from_utf8_lossy(&out.stdout);
    Some(parse_ps_time(raw.trim()))
}

/// Parse a `ps`-style time string `[[DD-]HH:]MM:SS` into seconds.
fn parse_ps_time(s: &str) -> f64 {
    // Split off an optional leading `DD-` day component.
    let (days, hms) = match s.split_once('-') {
        Some((d, rest)) => (d.parse::<f64>().unwrap_or(0.0), rest),
        None => (0.0, s),
    };
    let parts: Vec<f64> = hms.split(':').map(|p| p.parse().unwrap_or(0.0)).collect();
    let hms_secs = match parts.as_slice() {
        [h, m, sec] => h * 3600.0 + m * 60.0 + sec,
        [m, sec] => m * 60.0 + sec,
        [sec] => *sec,
        _ => 0.0,
    };
    days * 86_400.0 + hms_secs
}

#[cfg(test)]
mod tests {
    use super::parse_ps_time;

    // Pure parsing logic for the idle-CPU sampler — runs without NATS.
    #[test]
    fn parses_ps_time_formats() {
        assert!((parse_ps_time("00:03.50") - 3.5).abs() < 1e-9);
        assert!((parse_ps_time("01:30") - 90.0).abs() < 1e-9);
        assert!((parse_ps_time("1:00:00") - 3600.0).abs() < 1e-9);
        assert!((parse_ps_time("1-00:00:00") - 86_400.0).abs() < 1e-9);
        assert_eq!(parse_ps_time(""), 0.0);
    }
}
