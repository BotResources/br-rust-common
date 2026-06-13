#![allow(dead_code)] // shared test-helper module; each binary uses only a subset

use br_core_integration::{EventMetadata, IntegrationCommand, IntegrationEvent};
use br_core_kernel::{Actor, UserId};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TestPayload {
    pub label: String,
}

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

pub fn unique_prefix() -> String {
    format!("br_test_{}", Uuid::now_v7().simple())
}

pub fn metadata(correlation_id: Uuid) -> EventMetadata {
    EventMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation_id)
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

pub async fn jetstream() -> async_nats::jetstream::Context {
    let url = nats_url().expect("NATS_URL set");
    let client = async_nats::connect(&url).await.expect("connect to NATS");
    async_nats::jetstream::new(client)
}

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
            ack_wait: std::time::Duration::from_secs(2),
            ..Default::default()
        })
        .await
        .expect("create durable consumer");
}

pub async fn teardown(js: &async_nats::jetstream::Context, prefix: &str) {
    let name = format!("STREAM_{prefix}");
    if let Err(e) = js.delete_stream(&name).await {
        eprintln!("teardown: failed to delete stream {name}: {e}");
    }
}

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

fn parse_ps_time(s: &str) -> f64 {
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

    #[test]
    fn parses_ps_time_formats() {
        assert!((parse_ps_time("00:03.50") - 3.5).abs() < 1e-9);
        assert!((parse_ps_time("01:30") - 90.0).abs() < 1e-9);
        assert!((parse_ps_time("1:00:00") - 3600.0).abs() < 1e-9);
        assert!((parse_ps_time("1-00:00:00") - 86_400.0).abs() < 1e-9);
        assert_eq!(parse_ps_time(""), 0.0);
    }
}
