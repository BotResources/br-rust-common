//! Axum middleware that decodes the `X-Passport` header into a typed
//! [`br_core_auth::Passport`] request extension.
//!
//! Returns `401 Unauthorized` for a missing, empty, or malformed header.

use axum::body::Body;
use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use br_core_auth::{Passport, PassportHeader};

/// Axum middleware that extracts the `X-Passport` header, decodes the
/// base64-encoded JSON into a [`Passport`], and inserts it as a request
/// extension.
///
/// Returns 401 if the header is missing, empty, or malformed.
pub async fn passport_header_middleware(mut request: Request<Body>, next: Next) -> Response {
    let header_val = match request.headers().get("X-Passport") {
        Some(v) => match v.to_str() {
            Ok(s) if !s.is_empty() => s.to_string(),
            _ => return (StatusCode::UNAUTHORIZED, "missing or empty X-Passport").into_response(),
        },
        None => return (StatusCode::UNAUTHORIZED, "missing X-Passport header").into_response(),
    };

    let passport = match Passport::from_header(&header_val) {
        Ok(p) => p,
        Err(_) => return (StatusCode::UNAUTHORIZED, "malformed X-Passport header").into_response(),
    };

    request.extensions_mut().insert(passport);
    next.run(request).await
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
}
