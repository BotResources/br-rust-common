use std::collections::BTreeMap;

use crate::error::ErrorCode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeError {
    code: ErrorCode,
    reason_code: Option<String>,
    params: BTreeMap<String, String>,
    detail: Option<String>,
}

impl EdgeError {
    fn bare(code: ErrorCode) -> Self {
        Self {
            code,
            reason_code: None,
            params: BTreeMap::new(),
            detail: None,
        }
    }

    pub fn unauthenticated() -> Self {
        Self::bare(ErrorCode::Unauthenticated)
    }

    pub fn forbidden() -> Self {
        Self::bare(ErrorCode::Forbidden)
    }

    pub fn not_found() -> Self {
        Self::bare(ErrorCode::NotFound)
    }

    pub fn conflict() -> Self {
        Self::bare(ErrorCode::Conflict)
    }

    pub fn bad_user_input() -> Self {
        Self::bare(ErrorCode::BadUserInput)
    }

    pub fn invalid_state() -> Self {
        Self::bare(ErrorCode::InvalidState)
    }

    pub fn internal(detail: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            reason_code: None,
            params: BTreeMap::new(),
            detail: Some(detail.into()),
        }
    }

    #[must_use]
    pub fn with_reason(mut self, reason_code: impl Into<String>) -> Self {
        self.reason_code = Some(reason_code.into());
        self
    }

    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    pub fn code(&self) -> ErrorCode {
        self.code
    }

    pub fn reason_code(&self) -> Option<&str> {
        self.reason_code.as_deref()
    }

    pub fn params(&self) -> &BTreeMap<String, String> {
        &self.params
    }

    pub(crate) fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_constructors_set_their_code() {
        assert_eq!(EdgeError::forbidden().code(), ErrorCode::Forbidden);
        assert_eq!(EdgeError::not_found().code(), ErrorCode::NotFound);
        assert_eq!(EdgeError::conflict().code(), ErrorCode::Conflict);
        assert_eq!(EdgeError::bad_user_input().code(), ErrorCode::BadUserInput);
        assert_eq!(EdgeError::invalid_state().code(), ErrorCode::InvalidState);
        assert_eq!(
            EdgeError::unauthenticated().code(),
            ErrorCode::Unauthenticated
        );
    }

    #[test]
    fn reason_and_params_are_carried() {
        let err = EdgeError::conflict()
            .with_reason("name_already_taken")
            .with_param("name", "Acme");
        assert_eq!(err.reason_code(), Some("name_already_taken"));
        assert_eq!(err.params().get("name").map(String::as_str), Some("Acme"));
    }

    #[test]
    fn internal_carries_detail_for_logging() {
        let err = EdgeError::internal("sqlx: connection reset by peer");
        assert_eq!(err.code(), ErrorCode::Internal);
        assert_eq!(err.detail(), Some("sqlx: connection reset by peer"));
        assert_eq!(err.reason_code(), None);
    }
}
