//! Render an [`EdgeError`] into an Axum HTTP response, for the REST edge of a
//! service (e.g. `/internal/passport`, webhooks, health-adjacent endpoints).
//!
//! Body shape (stable, mirrors the GraphQL extensions contract):
//! ```json
//! { "error": { "code": "CONFLICT", "reason": "name_already_taken",
//!              "params": { "name": "Acme" } } }
//! ```
//! `reason` / `params` are omitted when absent. The HTTP status is
//! [`ErrorCode::http_status`]. An [`ErrorCode::Internal`] logs its detail and
//! returns a bare `{ "error": { "code": "INTERNAL" } }` — no server detail
//! reaches the client.

use axum::Json;
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value, json};

use crate::error::{EdgeError, ErrorCode};

impl IntoResponse for EdgeError {
    fn into_response(self) -> Response {
        if self.code() == ErrorCode::Internal
            && let Some(detail) = self.detail()
        {
            tracing::error!(error = detail, "internal error");
        }

        let status = self.code().http_status();

        let mut error = Map::new();
        error.insert("code".into(), json!(self.code().as_str()));
        if let Some(reason) = self.reason_code() {
            error.insert("reason".into(), json!(reason));
        }
        if !self.params().is_empty() {
            let params: Map<String, Value> = self
                .params()
                .iter()
                .map(|(k, v)| (k.clone(), json!(v)))
                .collect();
            error.insert("params".into(), Value::Object(params));
        }

        (status, Json(json!({ "error": error }))).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::StatusCode;

    async fn body_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    // Given a conflict with a reason + param, When turned into a response, Then
    // the status is 409 and the body carries code/reason/params.
    #[tokio::test]
    async fn conflict_renders_status_and_full_body() {
        let response = EdgeError::conflict()
            .with_reason("name_already_taken")
            .with_param("name", "Acme")
            .into_response();
        assert_eq!(response.status(), StatusCode::CONFLICT);

        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "CONFLICT");
        assert_eq!(body["error"]["reason"], "name_already_taken");
        assert_eq!(body["error"]["params"]["name"], "Acme");
    }

    // Given a bare not-found, Then reason/params are omitted (not null).
    #[tokio::test]
    async fn bare_error_omits_reason_and_params() {
        let response = EdgeError::not_found().into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "NOT_FOUND");
        assert!(body["error"].get("reason").is_none());
        assert!(body["error"].get("params").is_none());
    }

    // Given an internal fault, Then 500 + only the code; the detail is absent.
    #[tokio::test]
    async fn internal_returns_500_and_no_detail() {
        let response = EdgeError::internal("sqlx: connection reset").into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);

        let body = body_json(response).await;
        assert_eq!(body["error"]["code"], "INTERNAL");
        // The server detail is nowhere in the serialized body.
        assert!(!body.to_string().contains("connection reset"));
    }
}
