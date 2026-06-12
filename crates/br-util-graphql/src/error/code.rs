//! [`ErrorCode`] — the canonical, cross-service error-code contract.
//!
//! This code set is a **published contract every BR frontend binds to**: a
//! mutation/query failure surfaces one of these codes in the GraphQL
//! `extensions.code` field (and as the `code` of a REST error body), and the
//! client maps the code → localized copy. The strings are therefore **stable
//! keys, never UI prose** (codes-not-language); renaming one is a breaking
//! change across the whole estate, not a local edit.
//!
//! Before this crate, ~six diverging copies of this enum lived across the
//! services (each `svc-*/src/error.rs`); unifying them here makes the set the
//! single source of truth.

use axum::http::StatusCode;

/// The canonical edge error code. Each variant maps to exactly one stable wire
/// string (`as_str`) and one REST HTTP status (`http_status`).
///
/// Total and `Copy` — adding a variant forces every `match` on it to be
/// updated (no `_ =>` fall-through in the mappings), so the contract can never
/// silently grow a hole.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    /// No (or invalid) authenticated principal — the caller is not signed in.
    /// REST: 401.
    Unauthenticated,
    /// The principal is known but not permitted to perform the operation.
    /// REST: 403.
    Forbidden,
    /// The addressed resource does not exist (or is not visible to the caller).
    /// REST: 404.
    NotFound,
    /// The operation conflicts with current state (uniqueness, already-done).
    /// REST: 409.
    Conflict,
    /// The input is malformed or fails validation (a value-object rejection,
    /// a bad argument). REST: 400.
    BadUserInput,
    /// The aggregate is in a state that forbids this transition (a guarded
    /// state-machine rejection). REST: 422.
    InvalidState,
    /// An unexpected server-side fault. The detail is logged, never returned —
    /// the client sees only this code. REST: 500.
    Internal,
}

impl ErrorCode {
    /// The stable wire string carried in `extensions.code` / the REST `code`
    /// field. This is the contract the frontends key on — do not change a
    /// returned string without treating it as a breaking change.
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

    /// The REST HTTP status this code maps to, for the [`IntoResponse`] edge.
    ///
    /// [`IntoResponse`]: axum::response::IntoResponse
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

    // The wire strings are the contract — lock every one against accidental
    // rename. A change here is a deliberate, breaking, cross-estate migration.
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

    // codes-not-language: the wire string is an UPPER_SNAKE key, never a
    // sentence (no spaces, no lowercase prose).
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

    // Given each code, Then it maps to its documented REST status.
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

    /// Every variant, so the wire/codes tests iterate the whole contract. Kept
    /// in the test module (the lib does not need a runtime variant list).
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
