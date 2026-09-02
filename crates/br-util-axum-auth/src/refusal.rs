use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};

pub(crate) const UNAUTHORIZED_BODY: &str = "unauthorized";

pub(crate) const UNAUTHORIZED_GRAPHQL_BODY: &str =
    r#"{"errors":[{"message":"unauthorized","extensions":{"code":"UNAUTHENTICATED"}}]}"#;

pub(crate) fn text_unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, UNAUTHORIZED_BODY).into_response()
}

pub(crate) fn graphql_unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        [(header::CONTENT_TYPE, "application/json")],
        UNAUTHORIZED_GRAPHQL_BODY,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_util_graphql::ErrorCode;

    #[test]
    fn graphql_body_carries_the_canonical_unauthenticated_code() {
        let parsed: serde_json::Value = serde_json::from_str(UNAUTHORIZED_GRAPHQL_BODY).unwrap();

        assert_eq!(
            parsed["errors"][0]["extensions"]["code"],
            serde_json::Value::from(ErrorCode::Unauthenticated.as_str())
        );
        assert_eq!(
            parsed["errors"][0]["message"],
            serde_json::Value::from(UNAUTHORIZED_BODY)
        );
        assert_eq!(parsed["errors"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn graphql_refusal_is_json_typed_401() {
        let response = graphql_unauthorized();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::CONTENT_TYPE).unwrap(),
            "application/json"
        );
    }

    #[test]
    fn text_refusal_is_plain_401() {
        assert_eq!(text_unauthorized().status(), StatusCode::UNAUTHORIZED);
    }
}
