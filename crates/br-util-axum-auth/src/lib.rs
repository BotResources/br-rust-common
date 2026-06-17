use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use br_core_auth::{Passport, PassportHeader};

const UNAUTHORIZED_BODY: &str = "unauthorized";

pub async fn passport_header_middleware(mut request: Request<Body>, next: Next) -> Response {
    let header_val = match request.headers().get("X-Passport") {
        Some(v) => match v.to_str() {
            Ok(s) if !s.is_empty() => s.to_string(),
            Ok(_) => {
                tracing::warn!("X-Passport rejected: header present but empty");
                return unauthorized();
            }
            Err(_) => {
                tracing::warn!("X-Passport rejected: header value is not valid UTF-8");
                return unauthorized();
            }
        },
        None => {
            tracing::warn!("X-Passport rejected: header missing");
            return unauthorized();
        }
    };

    let passport = match Passport::from_header(&header_val) {
        Ok(p) => p,
        Err(_) => {
            tracing::warn!("X-Passport rejected: header could not be decoded");
            return unauthorized();
        }
    };

    request.extensions_mut().insert(passport);
    next.run(request).await
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use br_core_auth::{AuthMethod, PassportClaims};
    use tower::ServiceExt;
    use uuid::Uuid;

    fn test_router() -> Router {
        Router::new()
            .route(
                "/test",
                get(|passport: Option<axum::Extension<Passport>>| async move {
                    match passport {
                        Some(axum::Extension(p)) => format!("{}", p.actor_id()),
                        None => "no passport".to_string(),
                    }
                }),
            )
            .layer(axum::middleware::from_fn(passport_header_middleware))
    }

    fn make_passport_header() -> String {
        let p = Passport::human(
            Uuid::nil(),
            false,
            true,
            AuthMethod::Jwt,
            None,
            PassportClaims::new(),
        );
        p.to_header()
    }

    #[tokio::test]
    async fn valid_passport_header_passes_through() {
        let app = test_router();
        let req = Request::builder()
            .uri("/test")
            .header("X-Passport", make_passport_header())
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_header_returns_401() {
        let app = test_router();
        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn empty_header_returns_401() {
        let app = test_router();
        let req = Request::builder()
            .uri("/test")
            .header("X-Passport", "")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn malformed_header_returns_401() {
        let app = test_router();
        let req = Request::builder()
            .uri("/test")
            .header("X-Passport", "not-valid-base64!!!")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    async fn run(req: Request<Body>) -> (StatusCode, Vec<u8>) {
        let resp = test_router().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, bytes)
    }

    #[tokio::test]
    async fn all_rejection_causes_return_identical_opaque_body() {
        let missing = Request::builder().uri("/test").body(Body::empty()).unwrap();
        let empty = Request::builder()
            .uri("/test")
            .header("X-Passport", "")
            .body(Body::empty())
            .unwrap();
        let non_utf8 = Request::builder()
            .uri("/test")
            .header(
                "X-Passport",
                axum::http::HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap(),
            )
            .body(Body::empty())
            .unwrap();
        let malformed = Request::builder()
            .uri("/test")
            .header("X-Passport", "not-valid-base64!!!")
            .body(Body::empty())
            .unwrap();

        let (s_missing, b_missing) = run(missing).await;
        let (s_empty, b_empty) = run(empty).await;
        let (s_non_utf8, b_non_utf8) = run(non_utf8).await;
        let (s_malformed, b_malformed) = run(malformed).await;

        assert_eq!(s_missing, StatusCode::UNAUTHORIZED);
        assert_eq!(s_empty, StatusCode::UNAUTHORIZED);
        assert_eq!(s_non_utf8, StatusCode::UNAUTHORIZED);
        assert_eq!(s_malformed, StatusCode::UNAUTHORIZED);

        assert_eq!(b_missing, b_empty);
        assert_eq!(b_missing, b_non_utf8);
        assert_eq!(b_missing, b_malformed);
        assert_eq!(b_missing, UNAUTHORIZED_BODY.as_bytes());
    }
}
