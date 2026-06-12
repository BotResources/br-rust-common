//! [`EdgeError`] — the application-layer error that sits between a domain error
//! and the transport (GraphQL / REST).
//!
//! It is the unification point of the ~six diverging `AppError` copies across
//! the estate. It carries:
//! - a canonical [`ErrorCode`] (the cross-service class the frontend keys on);
//! - an optional `reason_code` — the **precise domain rule** that was broken
//!   (`name_already_taken`, `last_admin`, …), a stable code never UI prose, so
//!   the client can render a message more specific than the broad class;
//! - structured `params` for that reason (interpolation values for the copy);
//! - an internal `detail` that is **logged, never sent** — the place SQL /
//!   serialization / panic text is sanitized away before it can reach a client.
//!
//! A domain crate maps its own typed error into an `EdgeError` (`code` +
//! `reason_code` from the broken rule); the edge then renders it once, the same
//! way, for every service.

use std::collections::BTreeMap;

use crate::error::ErrorCode;

/// An application-layer error ready for the transport edge.
///
/// Build it with the class constructors ([`unauthenticated`](Self::unauthenticated),
/// [`forbidden`](Self::forbidden), …) then optionally attach the precise
/// `reason_code` + `params`. The [`internal`](Self::internal) path takes a
/// detail string that is logged at render time and **never** surfaced to the
/// client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeError {
    code: ErrorCode,
    /// The precise, stable reason code (the broken domain rule), if any. Never
    /// a sentence — the client maps it to localized copy.
    reason_code: Option<String>,
    /// Structured params for the reason code (copy interpolation values).
    params: BTreeMap<String, String>,
    /// Server-side detail for [`ErrorCode::Internal`]: logged on render, never
    /// returned to the client.
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

    /// The caller is not authenticated (no/invalid principal).
    pub fn unauthenticated() -> Self {
        Self::bare(ErrorCode::Unauthenticated)
    }

    /// The principal is not permitted to perform the operation.
    pub fn forbidden() -> Self {
        Self::bare(ErrorCode::Forbidden)
    }

    /// The addressed resource does not exist / is not visible.
    pub fn not_found() -> Self {
        Self::bare(ErrorCode::NotFound)
    }

    /// The operation conflicts with current state (uniqueness, already-done).
    pub fn conflict() -> Self {
        Self::bare(ErrorCode::Conflict)
    }

    /// The input is malformed or fails validation.
    pub fn bad_user_input() -> Self {
        Self::bare(ErrorCode::BadUserInput)
    }

    /// The aggregate state forbids this transition.
    pub fn invalid_state() -> Self {
        Self::bare(ErrorCode::InvalidState)
    }

    /// An unexpected server fault. `detail` is logged on render and **never**
    /// returned — clients only ever see the [`ErrorCode::Internal`] code.
    pub fn internal(detail: impl Into<String>) -> Self {
        Self {
            code: ErrorCode::Internal,
            reason_code: None,
            params: BTreeMap::new(),
            detail: Some(detail.into()),
        }
    }

    /// Attach the precise reason code (the broken domain rule). Builder-style.
    #[must_use]
    pub fn with_reason(mut self, reason_code: impl Into<String>) -> Self {
        self.reason_code = Some(reason_code.into());
        self
    }

    /// Attach a single structured param for the reason code. Builder-style;
    /// call repeatedly to add several.
    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(key.into(), value.into());
        self
    }

    /// The canonical error class.
    pub fn code(&self) -> ErrorCode {
        self.code
    }

    /// The precise reason code, if attached.
    pub fn reason_code(&self) -> Option<&str> {
        self.reason_code.as_deref()
    }

    /// The structured params for the reason code.
    pub fn params(&self) -> &BTreeMap<String, String> {
        &self.params
    }

    /// The internal detail (for the render layer to log; never sent).
    pub(crate) fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given a class constructor, Then the code is set and nothing leaks.
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

    // Given a reason + params, Then both are carried for the edge to render.
    #[test]
    fn reason_and_params_are_carried() {
        let err = EdgeError::conflict()
            .with_reason("name_already_taken")
            .with_param("name", "Acme");
        assert_eq!(err.reason_code(), Some("name_already_taken"));
        assert_eq!(err.params().get("name").map(String::as_str), Some("Acme"));
    }

    // The internal detail is held for logging but is a crate-internal field —
    // it must never be part of what a client can read (proven at the render
    // edge in `rest`/`gql`).
    #[test]
    fn internal_carries_detail_for_logging() {
        let err = EdgeError::internal("sqlx: connection reset by peer");
        assert_eq!(err.code(), ErrorCode::Internal);
        assert_eq!(err.detail(), Some("sqlx: connection reset by peer"));
        // No reason_code is exposed for an internal fault.
        assert_eq!(err.reason_code(), None);
    }
}
