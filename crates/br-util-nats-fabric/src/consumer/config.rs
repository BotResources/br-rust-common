use std::time::Duration;

use async_nats::jetstream::consumer::pull::Config;
use async_nats::jetstream::consumer::{AckPolicy, DeliverPolicy, ReplayPolicy};

pub(crate) const ACK_WAIT: Duration = Duration::from_secs(30);
pub(crate) const MAX_ACK_PENDING: i64 = 256;
pub(crate) const MAX_DELIVER: i64 = -1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsumerTuning {
    pub ack_wait: Duration,
    pub max_ack_pending: i64,
}

impl Default for ConsumerTuning {
    fn default() -> Self {
        Self {
            ack_wait: ACK_WAIT,
            max_ack_pending: MAX_ACK_PENDING,
        }
    }
}

pub(crate) fn durable_config(durable: &str, filters: &[&str], tuning: &ConsumerTuning) -> Config {
    let mut config = Config {
        durable_name: Some(durable.to_string()),
        ack_policy: AckPolicy::Explicit,
        ack_wait: tuning.ack_wait,
        max_ack_pending: tuning.max_ack_pending,
        max_deliver: MAX_DELIVER,
        deliver_policy: DeliverPolicy::All,
        replay_policy: ReplayPolicy::Instant,
        ..Config::default()
    };
    match filters {
        [single] => config.filter_subject = (*single).to_string(),
        many => config.filter_subjects = many.iter().map(|s| s.to_string()).collect(),
    }
    config
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_single_filter_lands_on_filter_subject() {
        let config = durable_config(
            "svc-notifier",
            &["integration.cmd.notifier.x.y.v1"],
            &ConsumerTuning::default(),
        );
        assert_eq!(config.filter_subject, "integration.cmd.notifier.x.y.v1");
        assert!(config.filter_subjects.is_empty());
    }

    #[test]
    fn several_filters_land_on_filter_subjects() {
        let config = durable_config(
            "svc-pm-roster",
            &[
                "integration.evt.identity.user.created.v1",
                "integration.evt.identity.group.created.v1",
            ],
            &ConsumerTuning::default(),
        );
        assert!(config.filter_subject.is_empty());
        assert_eq!(config.filter_subjects.len(), 2);
    }

    #[test]
    fn carries_the_documented_behavioral_defaults() {
        let config = durable_config(
            "d",
            &["integration.evt.a.b.c.v1"],
            &ConsumerTuning::default(),
        );
        assert_eq!(config.durable_name.as_deref(), Some("d"));
        assert!(matches!(config.ack_policy, AckPolicy::Explicit));
        assert_eq!(config.ack_wait, Duration::from_secs(30));
        assert_eq!(config.max_ack_pending, 256);
        assert_eq!(config.max_deliver, -1);
        assert!(matches!(config.deliver_policy, DeliverPolicy::All));
        assert!(matches!(config.replay_policy, ReplayPolicy::Instant));
    }

    #[test]
    fn default_tuning_is_the_documented_defaults() {
        let tuning = ConsumerTuning::default();
        assert_eq!(tuning.ack_wait, Duration::from_secs(30));
        assert_eq!(tuning.max_ack_pending, 256);
    }

    #[test]
    fn custom_tuning_threads_into_the_config_leaving_the_rest_fixed() {
        let tuning = ConsumerTuning {
            ack_wait: Duration::from_secs(120),
            max_ack_pending: 32,
        };
        let config = durable_config("d", &["integration.evt.a.b.c.v1"], &tuning);
        assert_eq!(config.ack_wait, Duration::from_secs(120));
        assert_eq!(config.max_ack_pending, 32);
        assert_eq!(config.max_deliver, -1);
        assert!(matches!(config.ack_policy, AckPolicy::Explicit));
        assert!(matches!(config.deliver_policy, DeliverPolicy::All));
        assert!(matches!(config.replay_policy, ReplayPolicy::Instant));
    }
}
