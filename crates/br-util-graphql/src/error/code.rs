use axum::http::StatusCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    Unauthenticated,
    Forbidden,
    NotFound,
    Conflict,
    BadUserInput,
    InvalidState,
    Internal,
}

impl ErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            ErrorCode::Unauthenticated => "UNAUTHENTICATED",
            ErrorCode::Forbidden => "FORBIDDEN",
            ErrorCode::NotFound => "NOT_FOUND",
            ErrorCode::Conflict => "CONFLICT",
            ErrorCode::BadUserInput => "BAD_USER_INPUT",
            ErrorCode::InvalidState => "INVALID_STATE",
            ErrorCode::Internal => "INTERNAL",
        }
    }

    pub const fn http_status(self) -> StatusCode {
        match self {
            ErrorCode::Unauthenticated => StatusCode::UNAUTHORIZED,
            ErrorCode::Forbidden => StatusCode::FORBIDDEN,
            ErrorCode::NotFound => StatusCode::NOT_FOUND,
            ErrorCode::Conflict => StatusCode::CONFLICT,
            ErrorCode::BadUserInput => StatusCode::BAD_REQUEST,
            ErrorCode::InvalidState => StatusCode::UNPROCESSABLE_ENTITY,
            ErrorCode::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_strings_are_the_published_contract() {
        assert_eq!(ErrorCode::Unauthenticated.as_str(), "UNAUTHENTICATED");
        assert_eq!(ErrorCode::Forbidden.as_str(), "FORBIDDEN");
        assert_eq!(ErrorCode::NotFound.as_str(), "NOT_FOUND");
        assert_eq!(ErrorCode::Conflict.as_str(), "CONFLICT");
        assert_eq!(ErrorCode::BadUserInput.as_str(), "BAD_USER_INPUT");
        assert_eq!(ErrorCode::InvalidState.as_str(), "INVALID_STATE");
        assert_eq!(ErrorCode::Internal.as_str(), "INTERNAL");
    }

    #[test]
    fn wire_strings_are_codes_not_sentences() {
        for code in EVERY_CODE {
            let s = code.as_str();
            assert!(!s.contains(' '), "{s} contains a space — looks like prose");
            assert!(
                s.chars().all(|c| c.is_ascii_uppercase() || c == '_'),
                "{s} is not an UPPER_SNAKE code"
            );
        }
    }

    #[test]
    fn http_status_mapping_is_total_and_correct() {
        assert_eq!(
            ErrorCode::Unauthenticated.http_status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(ErrorCode::Forbidden.http_status(), StatusCode::FORBIDDEN);
        assert_eq!(ErrorCode::NotFound.http_status(), StatusCode::NOT_FOUND);
        assert_eq!(ErrorCode::Conflict.http_status(), StatusCode::CONFLICT);
        assert_eq!(
            ErrorCode::BadUserInput.http_status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            ErrorCode::InvalidState.http_status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        assert_eq!(
            ErrorCode::Internal.http_status(),
            StatusCode::INTERNAL_SERVER_ERROR
        );
    }

    const EVERY_CODE: [ErrorCode; 7] = [
        ErrorCode::Unauthenticated,
        ErrorCode::Forbidden,
        ErrorCode::NotFound,
        ErrorCode::Conflict,
        ErrorCode::BadUserInput,
        ErrorCode::InvalidState,
        ErrorCode::Internal,
    ];
}
