use axum::Router;
use axum::http::{HeaderValue, StatusCode, header::CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

use crate::metrics::init::MetricsHandle;

const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";

/// The path the Prometheus exposition is served on. Part of the ops contract:
/// a scrape configuration points at it.
pub const METRICS_PATH: &str = "/metrics";

/// The metrics handler: `GET`, `200 OK`, the Prometheus text exposition.
/// Prefer [`metrics_router`], which mounts it on [`METRICS_PATH`].
pub fn metrics_route<S>(handle: MetricsHandle) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    get(move || {
        let handle = handle.clone();
        async move {
            let body = handle.render();
            (
                StatusCode::OK,
                [(
                    CONTENT_TYPE,
                    HeaderValue::from_static(PROMETHEUS_CONTENT_TYPE),
                )],
                body,
            )
                .into_response()
        }
    })
}

/// A router serving [`metrics_route`] on [`METRICS_PATH`], to `merge` into the
/// service's router: the path comes from the constant, never from a literal
/// the service could mistype.
pub fn metrics_router<S>(handle: MetricsHandle) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(METRICS_PATH, metrics_route(handle))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::init::shared_test_handle;
    use axum::body::Body;
    use axum::http::{Request, header::CONTENT_TYPE};
    use tower::ServiceExt;

    async fn assert_serves_the_exposition(app: Router, path: &str) {
        let resp = app
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get(CONTENT_TYPE).unwrap(),
            PROMETHEUS_CONTENT_TYPE
        );

        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap();
        assert!(
            body.contains("process_"),
            "process collectors are present in the exposition: {body}"
        );
    }

    #[tokio::test]
    async fn metrics_route_serves_200_prometheus_text() {
        let app = Router::new().route("/metrics", metrics_route(shared_test_handle()));
        assert_serves_the_exposition(app, "/metrics").await;
    }

    #[tokio::test]
    async fn the_router_serves_the_exposition_on_the_contract_path() {
        assert_serves_the_exposition(metrics_router(shared_test_handle()), METRICS_PATH).await;
    }
}
