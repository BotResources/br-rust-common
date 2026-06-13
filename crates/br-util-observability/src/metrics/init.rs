use metrics_exporter_prometheus::{Matcher, PrometheusHandle};
use metrics_process::Collector;

use crate::metrics::error::MetricsError;
use crate::metrics::http_layer::{LATENCY_BUCKETS_SECONDS, REQUEST_DURATION_SECONDS};

#[derive(Clone)]
pub struct MetricsHandle {
    prometheus: PrometheusHandle,
    process: Collector,
}

impl MetricsHandle {
    pub(crate) fn new(prometheus: PrometheusHandle, process: Collector) -> Self {
        Self {
            prometheus,
            process,
        }
    }

    pub fn render(&self) -> String {
        self.process.collect();
        self.prometheus.run_upkeep();
        self.prometheus.render()
    }

    pub fn prometheus(&self) -> &PrometheusHandle {
        &self.prometheus
    }
}

pub fn init_metrics(component: &str) -> Result<MetricsHandle, MetricsError> {
    let prometheus = metrics_exporter_prometheus::PrometheusBuilder::new()
        .add_global_label("component", component.to_string())
        .set_buckets_for_metric(
            Matcher::Full(REQUEST_DURATION_SECONDS.to_string()),
            LATENCY_BUCKETS_SECONDS,
        )
        .map_err(|e| MetricsError::Buckets(e.to_string()))?
        .install_recorder()
        .map_err(|e| MetricsError::Install(e.to_string()))?;

    let process = Collector::default();
    process.describe();

    crate::metrics::http_layer::describe_http_metrics();

    Ok(MetricsHandle::new(prometheus, process))
}

#[cfg(test)]
pub(crate) fn shared_test_handle() -> MetricsHandle {
    use std::sync::OnceLock;
    static HANDLE: OnceLock<MetricsHandle> = OnceLock::new();
    HANDLE
        .get_or_init(|| init_metrics("br-util-observability-test").expect("recorder installs once"))
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_install_returns_install_error_never_panics() {
        let _ = shared_test_handle();

        match init_metrics("br-util-observability-test") {
            Err(MetricsError::Install(_)) => {}
            Err(other) => panic!("a recorder double-install must return Install, got {other:?}"),
            Ok(_) => panic!("a recorder double-install must not succeed"),
        }
    }
}
