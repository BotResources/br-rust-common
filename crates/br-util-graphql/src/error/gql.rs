use async_graphql::{Error, ErrorExtensions, Name, Value};

use crate::error::{EdgeError, ErrorCode};

impl EdgeError {
    pub fn into_gql(self) -> Error {
        if self.code() == ErrorCode::Internal
            && let Some(detail) = self.detail()
        {
            tracing::error!(error = detail, "internal error");
        }

        let code = self.code();
        let reason = self.reason_code().map(str::to_owned);
        let params: Option<Value> = (!self.params().is_empty()).then(|| {
            Value::Object(
                self.params()
                    .iter()
                    .map(|(k, v)| (Name::new(k), Value::String(v.clone())))
                    .collect(),
            )
        });

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

    #[test]
    fn internal_detail_never_reaches_the_client() {
        let gql: Error = EdgeError::internal("sqlx: password authentication failed").into_gql();
        assert_eq!(gql.message, "INTERNAL");
        let ext = gql.extensions.expect("extensions present");
        assert_eq!(ext.get("code"), Some(&Value::from("INTERNAL")));
        assert_eq!(ext.get("reason"), None);
    }
}
