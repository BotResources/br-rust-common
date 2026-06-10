//! [`ServiceKey`] — the validated `{service}` identifier of a declaring service.

use std::fmt;

use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, Serialize, Serializer};

use crate::error::KeyValidationError;

/// Maximum length of a service key, in bytes. A service key is also the
/// `{service}` segment of a [`ScopeKey`](crate::ScopeKey): [`ScopeKey::new`]
/// bounds that segment by this same limit (under the scope key's total budget of
/// 128), so every `ScopeKey`'s service segment is a *possible* `ServiceKey` and
/// a scope no service could ever own is rejected at construction.
///
/// [`ScopeKey::new`]: crate::ScopeKey::new
pub const SERVICE_KEY_MAX_LEN: usize = 64;

/// The validated identifier of a service: the `{service}` segment in a
/// `{service}:{capability}` scope key.
///
/// Intrinsic validation, enforced in [`ServiceKey::new`] (and re-run on
/// deserialization, so a malformed wire value fails closed): ASCII `[a-z0-9_]`
/// only, non-empty, at most [`SERVICE_KEY_MAX_LEN`] bytes. Illegal values are
/// unrepresentable — there is no unvalidated path to a `ServiceKey`.
///
/// Deserializing a bare `ServiceKey` fails closed with an **opaque `serde`
/// error**, intentionally unstructured: fail-closed is the property here. The
/// *structured* key-syntax reason
/// ([`InvalidScopeKey`](crate::ScopeDeclarationError::InvalidScopeKey)) is
/// produced only on the receiver-side raw-form validation path
/// ([`RawScopeDeclaration::validate`](crate::RawScopeDeclaration::validate)),
/// never here.
///
/// Like the kernel id types, this is deliberately **not** `Deref<Target = str>`:
/// reach the inner value through [`as_str`](ServiceKey::as_str) or the
/// `AsRef<str>` impl so every raw access is explicit and greppable.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServiceKey(String);

impl ServiceKey {
    /// Validate `value` and build a `ServiceKey`, or return why it is malformed.
    ///
    /// # Errors
    ///
    /// - [`KeyValidationError::Empty`] if `value` is empty.
    /// - [`KeyValidationError::TooLong`] if it exceeds [`SERVICE_KEY_MAX_LEN`].
    /// - [`KeyValidationError::InvalidCharset`] if it contains any byte outside
    ///   ASCII `[a-z0-9_]`.
    pub fn new(value: impl Into<String>) -> Result<Self, KeyValidationError> {
        let value = value.into();
        validate_segment(&value, SERVICE_KEY_MAX_LEN)?;
        Ok(Self(value))
    }

    /// The validated key as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validate one `[a-z0-9_]` segment (used by both the service key and each
/// segment of a scope key). Empty before charset so an empty segment reports
/// [`Empty`](KeyValidationError::Empty), not [`InvalidCharset`].
pub(crate) fn validate_segment(value: &str, max_len: usize) -> Result<(), KeyValidationError> {
    if value.is_empty() {
        return Err(KeyValidationError::Empty);
    }
    if value.len() > max_len {
        return Err(KeyValidationError::TooLong {
            max: max_len,
            actual: value.len(),
        });
    }
    if !value
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(KeyValidationError::InvalidCharset);
    }
    Ok(())
}

impl fmt::Display for ServiceKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for ServiceKey {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ServiceKey {
    type Error = KeyValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for ServiceKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

// Deserialization re-validates: a malformed wire value fails closed with a
// serde error (no fail-open parse, no unvalidated construction path).
impl<'de> Deserialize<'de> for ServiceKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given a charset-clean, non-empty, short value → a ServiceKey is built.
    #[test]
    fn accepts_valid_service_key() {
        let key = ServiceKey::new("notifier").unwrap();
        assert_eq!(key.as_str(), "notifier");
    }

    #[test]
    fn accepts_digits_and_underscores() {
        assert!(ServiceKey::new("svc_auth_2").is_ok());
    }

    // Given an empty value → rejected as Empty (never built).
    #[test]
    fn rejects_empty() {
        assert_eq!(ServiceKey::new(""), Err(KeyValidationError::Empty));
    }

    #[test]
    fn rejects_uppercase() {
        assert_eq!(
            ServiceKey::new("Notifier"),
            Err(KeyValidationError::InvalidCharset)
        );
    }

    #[test]
    fn rejects_non_ascii() {
        assert_eq!(
            ServiceKey::new("café"),
            Err(KeyValidationError::InvalidCharset)
        );
    }

    #[test]
    fn rejects_colon() {
        // A colon belongs between segments of a scope key, never inside a
        // service key.
        assert_eq!(
            ServiceKey::new("notifier:read"),
            Err(KeyValidationError::InvalidCharset)
        );
    }

    #[test]
    fn rejects_too_long() {
        let long = "a".repeat(SERVICE_KEY_MAX_LEN + 1);
        assert_eq!(
            ServiceKey::new(long),
            Err(KeyValidationError::TooLong {
                max: SERVICE_KEY_MAX_LEN,
                actual: SERVICE_KEY_MAX_LEN + 1
            })
        );
    }

    #[test]
    fn accepts_exactly_max_len() {
        let at_max = "a".repeat(SERVICE_KEY_MAX_LEN);
        assert!(ServiceKey::new(at_max).is_ok());
    }

    #[test]
    fn serde_roundtrip() {
        let key = ServiceKey::new("notifier").unwrap();
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"notifier\"");
        let back: ServiceKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    // Fail-closed: a malformed wire value must NOT deserialize into a ServiceKey.
    #[test]
    fn deserialize_rejects_malformed() {
        let err = serde_json::from_str::<ServiceKey>("\"Notifier\"");
        assert!(err.is_err());
    }
}
