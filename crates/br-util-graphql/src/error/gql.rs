//! Render an [`EdgeError`] into an `async_graphql::Error`.
//!
//! The class [`ErrorCode`] goes into `extensions.code` (the field every BR
//! frontend keys on); the precise `reason_code` + `params`, when present, go
//! into `extensions.reason` / `extensions.params` so the client can render copy
//! more specific than the broad class. An [`ErrorCode::Internal`] logs its
//! detail and returns **only** the code — SQL / serialization / panic text
//! never reaches the client.

use async_graphql::{Error, ErrorExtensions, Name, Value};

use crate::error::{EdgeError, ErrorCode};

impl EdgeError {
    /// Convert into an `async_graphql::Error` carrying the contract extensions.
    ///
    /// `extensions.code` is the canonical [`ErrorCode`] string; if a
    /// `reason_code` is attached it is set as `extensions.reason`, and any
    /// params as `extensions.params`. For [`ErrorCode::Internal`] the detail is
    /// logged here and dropped from the response.
    pub fn into_gql(self) -> Error {
        if self.code() == ErrorCode::Internal
            && let Some(detail) = self.detail()
        {
            tracing::error!(error = detail, "internal error");
        }

        let code = self.code();
        let reason = self.reason_code().map(str::to_owned);
        // Mirror the REST body: a nested `params` object (not flattened keys), so
        // a client reading either edge through one mapper sees the same shape.
        let params: Option<Value> = (!self.params().is_empty()).then(|| {
            Value::Object(
                self.params()
                    .iter()
                    .map(|(k, v)| (Name::new(k), Value::String(v.clone())))
                    .collect(),
            )
        });

        // The `message` is the stable code, not prose — the human text is
        // rendered client-side from the code (codes-not-language).
        Error::new(code.as_str()).extend_with(|_, ext| {
            ext.set("code", code.as_str());
            if let Some(reason) = &reason {
                ext.set("reason", reason.as_str());
            }
            if let Some(params) = &params {
                ext.set("params", params.clone());
            }
        })
    }
}

impl From<EdgeError> for Error {
    fn from(error: EdgeError) -> Self {
        error.into_gql()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given an EdgeError with a reason + param, When rendered, Then the code,
    // reason and params land in the GraphQL extensions; the message is the code.
    // `params` is a nested object, mirroring the REST body shape.
    #[test]
    fn extensions_carry_code_reason_and_params() {
        let gql: Error = EdgeError::conflict()
            .with_reason("name_already_taken")
            .with_param("name", "Acme")
            .into_gql();

        assert_eq!(gql.message, "CONFLICT");
        let ext = gql.extensions.expect("extensions present");
        assert_eq!(ext.get("code"), Some(&Value::from("CONFLICT")));
        assert_eq!(ext.get("reason"), Some(&Value::from("name_already_taken")));
        let expected_params = Value::Object(
            [(Name::new("name"), Value::from("Acme"))]
                .into_iter()
                .collect(),
        );
        assert_eq!(ext.get("params"), Some(&expected_params));
    }

    // Given an internal error, When rendered, Then only the code surfaces — the
    // detail is never placed in the response extensions.
    #[test]
    fn internal_detail_never_reaches_the_client() {
        let gql: Error = EdgeError::internal("sqlx: password authentication failed").into_gql();
        assert_eq!(gql.message, "INTERNAL");
        let ext = gql.extensions.expect("extensions present");
        assert_eq!(ext.get("code"), Some(&Value::from("INTERNAL")));
        // The detail string appears nowhere a client can read.
        assert_eq!(ext.get("reason"), None);
    }
}
