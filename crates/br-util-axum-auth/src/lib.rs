mod refusal;

use axum::body::Body;
use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

use br_core_auth::{Passport, PassportHeader};

pub async fn passport_header_middleware(request: Request<Body>, next: Next) -> Response {
    admit(request, next, refusal::text_unauthorized).await
}

pub async fn passport_header_graphql_middleware(request: Request<Body>, next: Next) -> Response {
    admit(request, next, refusal::graphql_unauthorized).await
}

async fn admit(mut request: Request<Body>, next: Next, refuse: fn() -> Response) -> Response {
    match decode_passport(&request) {
        Some(passport) => {
            request.extensions_mut().insert(passport);
            next.run(request).await
        }
        None => refuse(),
    }
}

fn decode_passport(request: &Request<Body>) -> Option<Passport> {
    let header_val = match request.headers().get("X-Passport") {
        Some(v) => match v.to_str() {
            Ok(s) if !s.is_empty() => s,
            Ok(_) => {
                tracing::warn!("X-Passport rejected: header present but empty");
                return None;
            }
            Err(_) => {
                tracing::warn!("X-Passport rejected: header value is not valid UTF-8");
                return None;
            }
        },
        None => {
            tracing::warn!("X-Passport rejected: header missing");
            return None;
        }
    };

    match Passport::from_header(header_val) {
        Ok(p) => Some(p),
        Err(_) => {
            tracing::warn!("X-Passport rejected: header could not be decoded");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{HeaderValue, Request, StatusCode, header};
    use axum::routing::get;
    use br_core_auth::{AuthMethod, PassportClaims};
    use refusal::{UNAUTHORIZED_BODY, UNAUTHORIZED_GRAPHQL_BODY};
    use tower::ServiceExt;
    use uuid::Uuid;

    fn echo_actor_route() -> Router {
        Router::new().route(
            "/test",
            get(|passport: Option<axum::Extension<Passport>>| async move {
                match passport {
                    Some(axum::Extension(p)) => format!("{}", p.actor_id()),
                    None => "no passport".to_string(),
                }
            }),
        )
    }

    fn text_router() -> Router {
        echo_actor_route().layer(axum::middleware::from_fn(passport_header_middleware))
    }

    fn graphql_router() -> Router {
        echo_actor_route().layer(axum::middleware::from_fn(
            passport_header_graphql_middleware,
        ))
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

    fn authenticated_request() -> Request<Body> {
        Request::builder()
            .uri("/test")
            .header("X-Passport", make_passport_header())
            .body(Body::empty())
            .unwrap()
    }

    fn every_rejection_cause() -> Vec<(&'static str, Request<Body>)> {
        vec![
            (
                "missing",
                Request::builder().uri("/test").body(Body::empty()).unwrap(),
            ),
            (
                "empty",
                Request::builder()
                    .uri("/test")
                    .header("X-Passport", "")
                    .body(Body::empty())
                    .unwrap(),
            ),
            (
                "non-utf8",
                Request::builder()
                    .uri("/test")
                    .header(
                        "X-Passport",
                        HeaderValue::from_bytes(&[0xff, 0xfe]).unwrap(),
                    )
                    .body(Body::empty())
                    .unwrap(),
            ),
            (
                "undecodable",
                Request::builder()
                    .uri("/test")
                    .header("X-Passport", "not-valid-base64!!!")
                    .body(Body::empty())
                    .unwrap(),
            ),
        ]
    }

    async fn run(router: Router, req: Request<Body>) -> (StatusCode, Option<String>, Vec<u8>) {
        let resp = router.oneshot(req).await.unwrap();
        let status = resp.status();
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .map(|v| v.to_str().unwrap().to_string());
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap()
            .to_vec();
        (status, content_type, bytes)
    }

    #[tokio::test]
    async fn valid_passport_header_passes_through() {
        let (status, _, body) = run(text_router(), authenticated_request()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, Uuid::nil().to_string().as_bytes());
    }

    #[tokio::test]
    async fn valid_passport_header_passes_through_the_graphql_middleware() {
        let (status, _, body) = run(graphql_router(), authenticated_request()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, Uuid::nil().to_string().as_bytes());
    }

    #[tokio::test]
    async fn all_rejection_causes_return_identical_opaque_text_body() {
        for (cause, req) in every_rejection_cause() {
            let (status, content_type, body) = run(text_router(), req).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "cause: {cause}");
            assert_eq!(
                content_type.as_deref(),
                Some("text/plain; charset=utf-8"),
                "cause: {cause}"
            );
            assert_eq!(body, UNAUTHORIZED_BODY.as_bytes(), "cause: {cause}");
        }
    }

    #[tokio::test]
    async fn all_rejection_causes_return_identical_opaque_graphql_body() {
        for (cause, req) in every_rejection_cause() {
            let (status, content_type, body) = run(graphql_router(), req).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED, "cause: {cause}");
            assert_eq!(
                content_type.as_deref(),
                Some("application/json"),
                "cause: {cause}"
            );
            assert_eq!(body, UNAUTHORIZED_GRAPHQL_BODY.as_bytes(), "cause: {cause}");
        }
    }
}
