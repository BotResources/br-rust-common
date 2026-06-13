use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

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
