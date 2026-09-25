use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

/// The path liveness is served on. Part of the ops contract: the
/// `br-common-service` chart probes it by default (`probes.livenessPath`) and
/// is gated against this constant.
pub const LIVENESS_PATH: &str = "/livez";

/// The liveness handler: `GET`, always `200 OK`, body `alive`. Prefer
/// [`liveness_router`], which mounts it on [`LIVENESS_PATH`].
pub fn liveness_route<S>() -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    get(|| async { (StatusCode::OK, "alive").into_response() })
}

/// A router serving [`liveness_route`] on [`LIVENESS_PATH`], to `merge` into
/// the service's router: the path comes from the constant, never from a
/// literal the service could mistype.
pub fn liveness_router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().route(LIVENESS_PATH, liveness_route())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn get(app: Router, path: &str) -> (StatusCode, String) {
        let resp = app
            .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn livez_is_always_200_alive() {
        let app = Router::new().route("/livez", liveness_route());
        let (status, body) = get(app, "/livez").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "alive");
    }

    #[tokio::test]
    async fn the_router_serves_liveness_on_the_contract_path() {
        let (status, body) = get(liveness_router(), LIVENESS_PATH).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "alive");
    }

    #[tokio::test]
    async fn the_router_serves_nothing_else() {
        let (status, _) = get(liveness_router(), "/health").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }
}
