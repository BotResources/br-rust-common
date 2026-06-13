use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Instant;

use axum::extract::{MatchedPath, Request};
use axum::response::Response;
use metrics::{
    Unit, counter, describe_counter, describe_gauge, describe_histogram, gauge, histogram,
};
use tower::{Layer, Service};

pub const LATENCY_BUCKETS_SECONDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

const UNMATCHED_ROUTE: &str = "<unmatched>";

const REQUESTS_TOTAL: &str = "http_requests_total";
pub(crate) const REQUEST_DURATION_SECONDS: &str = "http_request_duration_seconds";
const REQUESTS_IN_FLIGHT: &str = "http_requests_in_flight";

#[derive(Clone, Copy, Default)]
pub struct HttpMetricsLayer;

pub fn http_metrics_layer() -> HttpMetricsLayer {
    HttpMetricsLayer
}

impl<S> Layer<S> for HttpMetricsLayer {
    type Service = HttpMetricsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        HttpMetricsService { inner }
    }
}

#[derive(Clone)]
pub struct HttpMetricsService<S> {
    inner: S,
}

struct InFlightGuard {
    method: String,
    route: String,
}

impl InFlightGuard {
    fn enter(method: String, route: String) -> Self {
        gauge!(REQUESTS_IN_FLIGHT, "method" => method.clone(), "route" => route.clone())
            .increment(1.0);
        Self { method, route }
    }
}

impl Drop for InFlightGuard {
    fn drop(&mut self) {
        gauge!(REQUESTS_IN_FLIGHT, "method" => self.method.clone(), "route" => self.route.clone())
            .decrement(1.0);
    }
}

impl<S> Service<Request> for HttpMetricsService<S>
where
    S: Service<Request, Response = Response> + Clone + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let method = request.method().as_str().to_string();
        let route = route_label(&request);

        let mut inner = self.inner.clone();
        Box::pin(async move {
            let _in_flight = InFlightGuard::enter(method.clone(), route.clone());
            let started = Instant::now();

            let response = inner.call(request).await?;

            let elapsed = started.elapsed().as_secs_f64();
            let status = response.status().as_u16().to_string();

            counter!(REQUESTS_TOTAL, "method" => method.clone(), "route" => route.clone(), "status" => status.clone())
                .increment(1);
            histogram!(REQUEST_DURATION_SECONDS, "method" => method, "route" => route, "status" => status)
                .record(elapsed);

            Ok(response)
        })
    }
}

fn route_label(request: &Request) -> String {
    request
        .extensions()
        .get::<MatchedPath>()
        .map_or_else(|| UNMATCHED_ROUTE.to_string(), |p| p.as_str().to_string())
}

pub(crate) fn describe_http_metrics() {
    describe_counter!(
        REQUESTS_TOTAL,
        Unit::Count,
        "Total HTTP requests by method, matched route template and status."
    );
    describe_histogram!(
        REQUEST_DURATION_SECONDS,
        Unit::Seconds,
        "HTTP request latency by method, matched route template and status."
    );
    describe_gauge!(
        REQUESTS_IN_FLIGHT,
        Unit::Count,
        "In-flight HTTP requests by method and matched route template."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::Request as HttpRequest;
    use axum::routing::get;
    use metrics_exporter_prometheus::PrometheusBuilder;
    use tower::ServiceExt;

    #[test]
    fn latency_buckets_are_sorted_and_finite() {
        assert!(!LATENCY_BUCKETS_SECONDS.is_empty());
        assert!(
            LATENCY_BUCKETS_SECONDS
                .windows(2)
                .all(|w| w[0] < w[1] && w[0].is_finite() && w[1].is_finite()),
            "buckets must be strictly increasing and finite"
        );
    }

    fn render_after_request(uri: &str) -> String {
        let recorder = PrometheusBuilder::new()
            .set_buckets(LATENCY_BUCKETS_SECONDS)
            .unwrap()
            .build_recorder();
        let handle = recorder.handle();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        metrics::with_local_recorder(&recorder, || {
            runtime.block_on(async {
                let app = Router::new()
                    .route("/users/{id}", get(|| async { "ok" }))
                    .layer(http_metrics_layer());
                app.oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
            });
            handle.render()
        })
    }

    #[test]
    fn a_parameterized_request_is_labeled_by_template_never_the_concrete_value() {
        let rendered = render_after_request("/users/12345");

        assert!(
            rendered.contains("route=\"/users/{id}\""),
            "the matched route template must be the label: {rendered}"
        );
        assert!(
            !rendered.contains("12345"),
            "the concrete path value must never appear in the exposition: {rendered}"
        );
        assert!(
            !rendered.contains("route=\"/users/12345\""),
            "the concrete path must never be a label: {rendered}"
        );
    }

    #[test]
    fn an_unmatched_request_fails_closed_to_the_sentinel_not_the_raw_path() {
        let rendered = render_after_request("/leaky-secret-9f8a7b6c");

        assert!(
            rendered.contains("route=\"<unmatched>\""),
            "an unmatched route must fall back to the constant sentinel: {rendered}"
        );
        assert!(
            !rendered.contains("9f8a7b6c"),
            "the raw unmatched path must never appear in the exposition: {rendered}"
        );
    }

    #[test]
    fn requests_and_latency_are_recorded_with_method_and_status() {
        let rendered = render_after_request("/users/7");

        assert!(rendered.contains("http_requests_total"));
        assert!(rendered.contains("http_request_duration_seconds"));
        assert!(rendered.contains("method=\"GET\""));
        assert!(rendered.contains("status=\"200\""));
    }

    fn in_flight_values(rendered: &str) -> Vec<f64> {
        rendered
            .lines()
            .filter(|l| l.starts_with(REQUESTS_IN_FLIGHT))
            .filter_map(|l| l.rsplit_once(' '))
            .filter_map(|(_, v)| v.trim().parse::<f64>().ok())
            .collect()
    }

    #[test]
    fn in_flight_returns_to_zero_when_the_inner_service_errors_before_completion() {
        let recorder = PrometheusBuilder::new().build_recorder();
        let handle = recorder.handle();

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let rendered = metrics::with_local_recorder(&recorder, || {
            runtime.block_on(async {
                let inner = tower::service_fn(|_req: Request| async {
                    Err::<Response, std::io::Error>(std::io::Error::other("boom"))
                });
                let mut service = http_metrics_layer().layer(inner);
                let request = HttpRequest::builder()
                    .uri("/users/{id}")
                    .body(Body::empty())
                    .unwrap();
                let result = service.call(request).await;
                assert!(
                    result.is_err(),
                    "the inner service must error so the post-await path is never reached"
                );
            });
            handle.render()
        });

        let values = in_flight_values(&rendered);
        assert!(
            !values.is_empty(),
            "the in-flight gauge must be present in the exposition: {rendered}"
        );
        assert!(
            values.iter().all(|v| *v == 0.0),
            "in-flight must be decremented on the error path (RAII Drop), got {values:?}: {rendered}"
        );
    }
}
