use axum::http::{HeaderValue, StatusCode, header::CONTENT_TYPE};
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

use crate::metrics::init::MetricsHandle;

const PROMETHEUS_CONTENT_TYPE: &str = "text/plain; version=0.0.4";

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::init::shared_test_handle;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, header::CONTENT_TYPE};
    use tower::ServiceExt;

    #[tokio::test]
    async fn metrics_route_serves_200_prometheus_text() {
        let app = Router::new().route("/metrics", metrics_route(shared_test_handle()));
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
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
}
