use async_nats::jetstream::consumer::PullConsumer;

use crate::classify::{classify_consumer_info, classify_get_stream};
use crate::error::{ConsumeErrorKind, FabricError};

pub(crate) async fn bind_durable(
    jetstream: &async_nats::jetstream::Context,
    stream_name: &'static str,
    durable: &str,
    expected_filter: &str,
) -> Result<PullConsumer, FabricError> {
    bind_durable_many(
        jetstream,
        stream_name,
        durable,
        std::slice::from_ref(&expected_filter),
    )
    .await
}

pub(crate) async fn bind_durable_many(
    jetstream: &async_nats::jetstream::Context,
    stream_name: &'static str,
    durable: &str,
    expected_filters: &[&str],
) -> Result<PullConsumer, FabricError> {
    let stream = jetstream
        .get_stream(stream_name)
        .await
        .map_err(|e| FabricError::consume(classify_get_stream(&e), e.to_string()))?;

    let consumer: PullConsumer = stream.get_consumer(durable).await.map_err(|e| {
        match e.downcast_ref::<async_nats::jetstream::context::ConsumerInfoError>() {
            Some(info_err) => {
                FabricError::consume(classify_consumer_info(info_err), info_err.to_string())
            }
            None => FabricError::consume(ConsumeErrorKind::Other, e.to_string()),
        }
    })?;

    verify_filter(
        stream_name,
        durable,
        expected_filters,
        &consumer.cached_info().config,
    )?;
    Ok(consumer)
}

fn verify_filter(
    stream_name: &'static str,
    durable: &str,
    expected_filters: &[&str],
    config: &async_nats::jetstream::consumer::Config,
) -> Result<(), FabricError> {
    let configured = configured_filters(config);
    let expected = expected_set(expected_filters);
    if !expected.is_empty() && filter_set(&configured) == expected {
        return Ok(());
    }
    Err(FabricError::FilterMismatch {
        stream: stream_name,
        durable: durable.to_string(),
        expected: expected_filters.join(", "),
        configured,
    })
}

fn filter_set(filters: &[String]) -> std::collections::BTreeSet<String> {
    filters.iter().cloned().collect()
}

fn expected_set(filters: &[&str]) -> std::collections::BTreeSet<String> {
    filters.iter().map(|s| s.to_string()).collect()
}

fn configured_filters(config: &async_nats::jetstream::consumer::Config) -> Vec<String> {
    if !config.filter_subjects.is_empty() {
        return config.filter_subjects.clone();
    }
    if config.filter_subject.is_empty() {
        return Vec::new();
    }
    vec![config.filter_subject.clone()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_nats::jetstream::consumer::Config;

    fn config_with(single: &str, many: &[&str]) -> Config {
        Config {
            filter_subject: single.to_string(),
            filter_subjects: many.iter().map(|s| s.to_string()).collect(),
            ..Config::default()
        }
    }

    #[test]
    fn single_filter_is_read_back() {
        let cfg = config_with("integration.evt.identity.user.created.v1", &[]);
        assert_eq!(
            configured_filters(&cfg),
            vec!["integration.evt.identity.user.created.v1".to_string()]
        );
    }

    #[test]
    fn multi_filter_takes_precedence_over_empty_single() {
        let cfg = config_with(
            "",
            &["integration.evt.a.b.c.v1", "integration.evt.a.b.d.v1"],
        );
        assert_eq!(configured_filters(&cfg).len(), 2);
    }

    #[test]
    fn matching_single_filter_passes_verification() {
        let cfg = config_with("integration.cmd.notifier.notification.deliver.v1", &[]);
        assert!(
            verify_filter(
                "INTEGRATION_CMD",
                "notifier",
                &["integration.cmd.notifier.notification.deliver.v1"],
                &cfg,
            )
            .is_ok()
        );
    }

    #[test]
    fn a_widened_durable_is_rejected() {
        let cfg = config_with("integration.evt.>", &[]);
        let err = verify_filter(
            "INTEGRATION_EVT",
            "svc-pm-users",
            &["integration.evt.identity.user.created.v1"],
            &cfg,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            FabricError::FilterMismatch { configured, .. } if configured == vec!["integration.evt.>".to_string()]
        ));
    }

    #[test]
    fn a_multi_subject_durable_is_rejected_when_it_does_not_match_a_single_expectation() {
        let cfg = config_with(
            "",
            &[
                "integration.evt.identity.user.created.v1",
                "integration.evt.identity.group.created.v1",
            ],
        );
        let err = verify_filter(
            "INTEGRATION_EVT",
            "svc-pm-users",
            &["integration.evt.identity.user.created.v1"],
            &cfg,
        )
        .unwrap_err();
        assert!(matches!(err, FabricError::FilterMismatch { .. }));
    }

    #[test]
    fn a_multi_subject_durable_passes_when_its_set_matches_order_insensitively() {
        let cfg = config_with(
            "",
            &[
                "integration.evt.identity.group.created.v1",
                "integration.evt.identity.user.created.v1",
            ],
        );
        assert!(
            verify_filter(
                "INTEGRATION_EVT",
                "svc-pm-roster",
                &[
                    "integration.evt.identity.user.created.v1",
                    "integration.evt.identity.group.created.v1",
                ],
                &cfg,
            )
            .is_ok()
        );
    }

    #[test]
    fn an_empty_expected_set_is_rejected_even_against_an_unfiltered_durable() {
        let cfg = config_with("", &[]);
        let err = verify_filter("INTEGRATION_EVT", "svc-pm-roster", &[], &cfg).unwrap_err();
        assert!(matches!(err, FabricError::FilterMismatch { .. }));
    }

    #[test]
    fn a_multi_subject_durable_missing_one_expected_subject_is_rejected() {
        let cfg = config_with("", &["integration.evt.identity.user.created.v1"]);
        let err = verify_filter(
            "INTEGRATION_EVT",
            "svc-pm-roster",
            &[
                "integration.evt.identity.user.created.v1",
                "integration.evt.identity.group.created.v1",
            ],
            &cfg,
        )
        .unwrap_err();
        assert!(matches!(err, FabricError::FilterMismatch { .. }));
    }
}
