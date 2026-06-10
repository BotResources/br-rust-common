//! Axum middleware that decodes the `X-Passport` header into a typed
//! [`br_core_auth::Passport`] request extension.
//!
//! Returns a uniform, opaque `401 Unauthorized` for a missing, empty, or
//! malformed header.

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use br_core_auth::{Passport, PassportHeader};

/// Constant body returned for every rejection. The precise cause is logged
/// server-side, never disclosed to the caller, so the 401 is opaque and gives
/// an unauthenticated caller no validation oracle.
const UNAUTHORIZED_BODY: &str = "unauthorized";

/// Axum middleware that extracts the `X-Passport` header, decodes the
/// base64-encoded JSON into a [`Passport`], and inserts it as a request
/// extension.
///
/// Returns a uniform, opaque `401 Unauthorized` (constant body
/// `"unauthorized"`) when the header is missing, empty, non-UTF8, or
/// malformed — every cause looks identical to the caller so the response is
/// not a validation oracle; the precise cause goes to `tracing::warn!`
/// server-side (the header value is never logged, as it may carry a forged
/// passport payload).
///
/// **Trust boundary.** `X-Passport` is trustworthy only because the gateway
/// strips any client-supplied copy and re-injects the resolved one, and
/// NetworkPolicy blocks direct external access. This middleware *decodes* the
/// header; it does not authenticate its origin — never expose a service
/// mounting it except behind the gateway.
pub async fn passport_header_middleware(mut request: Request<Body>, next: Next) -> Response {
    // SECURITY: never interpolate the header VALUE into any log line below —
    // it may contain a forged passport payload chosen by the caller.
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

/// The single, opaque 401 response shared by every rejection cause.
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
    use br_core_auth::AuthMethod;
    use serde_json::json;
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
        let p = Passport::Human {
            user_id: Uuid::nil(),
            is_super_admin: false,
            is_active: true,
            auth_method: AuthMethod::Jwt,
            impersonator: None,
            claims: json!({}),
        };
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

    /// Drive one request and return its `(status, body)`. A fresh router per
    /// request because `oneshot` consumes the service.
    async fn run(req: Request<Body>) -> (StatusCode, Vec<u8>) {
        let resp = test_router().oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, bytes)
    }

    // Given the four distinct rejection causes (missing / empty / non-UTF8 /
    // malformed header)
    // When each is rejected
    // Then the 401 body is byte-identical across all of them — the response
    // leaks nothing about which check failed (no validation oracle)
    #[tokio::test]
    async fn all_rejection_causes_return_identical_opaque_body() {
        // missing
        let missing = Request::builder().uri("/test").body(Body::empty()).unwrap();
        // empty
        let empty = Request::builder()
            .uri("/test")
            .header("X-Passport", "")
            .body(Body::empty())
            .unwrap();
        // non-UTF8 header value (raw bytes that are not valid UTF-8)
        let non_utf8 = Request::builder()
            .uri("/test")
            .header(
                "X-Passport",
                axum::http::HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap(),
            )
            .body(Body::empty())
            .unwrap();
        // malformed (valid UTF-8, not a decodable passport)
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
