//! Liveness — the always-200 probe a process exposes so the orchestrator can
//! tell the **process is alive** (distinct from *ready to serve*).

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

/// An Axum `GET` route that always answers `200 OK` (body `"alive"`).
///
/// **Liveness, not readiness.** This says "the process is up and its HTTP loop
/// is answering" — nothing about whether it should receive traffic *now*. That
/// distinction is deliberate and complementary:
///
/// - **Liveness** (this crate, `/livez`) → a failed probe means the process is
///   wedged and Kubernetes **restarts** it. So liveness must depend on nothing
///   but the HTTP loop itself: it is unconditionally `200`. Gating it on a
///   dependency would turn a transient outage into a crash-loop.
/// - **Readiness** (`br-util-axum-readiness`, `/readyz`) → a failed probe means
///   the process is alive but should be taken **out of rotation**, not killed.
///
/// Mount this alongside the readiness gate; never point a liveness probe at
/// `/readyz` (a slow dependency would restart a healthy process):
///
/// ```
/// use axum::Router;
/// use br_util_observability::liveness_route;
///
/// let app: Router = Router::new().route("/livez", liveness_route());
/// ```
///
/// Generic over the router state, so it mounts into any `Router<S>`.
pub fn liveness_route<S>() -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    get(|| async { (StatusCode::OK, "alive").into_response() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn get_livez(app: Router) -> (StatusCode, String) {
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/livez")
                    .body(Body::empty())
                    .unwrap(),
            )
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
        let (status, body) = get_livez(app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "alive");
    }
}
