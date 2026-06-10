//! [`ScopeKey`] — a validated `{service}:{capability}` permission key.

use std::fmt;

use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, Serialize, Serializer};

use crate::error::KeyValidationError;
use crate::service::{SERVICE_KEY_MAX_LEN, ServiceKey, validate_segment};

/// Maximum total length of a scope key, in bytes (both segments plus the `:`).
pub const SCOPE_KEY_MAX_LEN: usize = 128;

/// A validated permission key of the shape `{service}:{capability}`.
///
/// Scope keys are **dynamic, never a global enum**: each service declares its
/// own at runtime, so a fixed Rust enum could never enumerate them. The meaning
/// is carried by the validated type instead.
///
/// ## Intrinsic validation (in [`ScopeKey::new`], re-run on deserialization)
///
/// - ASCII `[a-z0-9_]` for every character *except* the single separator;
/// - exactly one `:`, splitting the key into `{service}` and `{capability}`;
/// - both segments non-empty;
/// - the `{service}` segment at most
///   [`SERVICE_KEY_MAX_LEN`](crate::SERVICE_KEY_MAX_LEN) bytes, so it is always a
///   *possible* [`ServiceKey`] — a scope no service could ever own is rejected;
/// - total length at most [`SCOPE_KEY_MAX_LEN`] bytes.
///
/// A malformed key is unrepresentable — there is no unvalidated path. Bare-key
/// deserialization re-runs this validation and **fails closed with an opaque
/// `serde` error** (intentionally unstructured: fail-closed is the property
/// here; the *structured* reason lives on the raw-form validation path —
/// [`RawScopeDeclaration::validate`](crate::RawScopeDeclaration::validate)).
///
/// ## What is *not* intrinsic
///
/// The rule "the `{service}` segment must equal the declaring service's key" is
/// **contextual** — it needs to know who is declaring — so it deliberately does
/// **not** live in the constructor (which never takes a declaring service).
/// Check it with [`is_owned_by`](ScopeKey::is_owned_by); it is enforced at
/// declaration assembly and surfaces as
/// [`ScopePrefixMismatch`](crate::ScopeDeclarationError::ScopePrefixMismatch).
///
/// Like the kernel id types, this is deliberately **not** `Deref`: reach the
/// inner value through [`as_str`](ScopeKey::as_str) or the `AsRef<str>` impl.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeKey {
    raw: String,
    /// Byte index of the single `:` separator, so the segment accessors are
    /// allocation-free and the invariant (exactly one colon) is captured once.
    colon: usize,
}

impl ScopeKey {
    /// Validate `value` and build a `ScopeKey`, or return why it is malformed.
    ///
    /// # Errors
    ///
    /// - [`KeyValidationError::TooLong`] if the total exceeds
    ///   [`SCOPE_KEY_MAX_LEN`];
    /// - [`KeyValidationError::MalformedSegments`] if there is not exactly one
    ///   `:` separating two segments;
    /// - [`KeyValidationError::Empty`] if either segment is empty;
    /// - [`KeyValidationError::InvalidCharset`] if either segment holds a byte
    ///   outside ASCII `[a-z0-9_]`.
    pub fn new(value: impl Into<String>) -> Result<Self, KeyValidationError> {
        let value = value.into();
        // Length first: bound the input before any scan.
        if value.len() > SCOPE_KEY_MAX_LEN {
            return Err(KeyValidationError::TooLong {
                max: SCOPE_KEY_MAX_LEN,
                actual: value.len(),
            });
        }
        // Exactly one `:`. `split_once` would accept two colons (the second
        // landing in the capability), so count explicitly.
        let mut parts = value.split(':');
        let (Some(service), Some(capability), None) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(KeyValidationError::MalformedSegments);
        };
        // Each segment must satisfy the shared `[a-z0-9_]`, non-empty rule. The
        // `{service}` segment is bounded by `SERVICE_KEY_MAX_LEN` (not the total
        // budget) so it is always a *possible* `ServiceKey`: a scope whose
        // service segment no `ServiceKey` could ever match — and so no service
        // could ever own — is rejected here as intrinsically inconsistent. The
        // capability is bounded by the total cap (the colon + non-empty service
        // already leave it strictly under the budget).
        validate_segment(service, SERVICE_KEY_MAX_LEN)?;
        validate_segment(capability, SCOPE_KEY_MAX_LEN)?;
        let colon = service.len();
        Ok(Self { raw: value, colon })
    }

    /// The full validated key as a string slice (`"{service}:{capability}"`).
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// The `{service}` segment (everything before the `:`).
    pub fn service_segment(&self) -> &str {
        &self.raw[..self.colon]
    }

    /// The `{capability}` segment (everything after the `:`).
    pub fn capability_segment(&self) -> &str {
        &self.raw[self.colon + 1..]
    }

    /// Whether this key's `{service}` segment matches `service` — i.e. whether
    /// `service` is allowed to declare this scope.
    ///
    /// This is the **contextual** ownership rule kept out of the constructor:
    /// validity of the key string is intrinsic, but who may declare it depends
    /// on the declarant, known only at declaration assembly.
    pub fn is_owned_by(&self, service: &ServiceKey) -> bool {
        self.service_segment() == service.as_str()
    }
}

impl fmt::Display for ScopeKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.raw)
    }
}

impl AsRef<str> for ScopeKey {
    fn as_ref(&self) -> &str {
        &self.raw
    }
}

impl TryFrom<String> for ScopeKey {
    type Error = KeyValidationError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl Serialize for ScopeKey {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.raw)
    }
}

// Deserialization re-validates: a malformed wire value fails closed with a
// serde error (no fail-open parse, no unvalidated construction path).
impl<'de> Deserialize<'de> for ScopeKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─── valid construction ───────────────────────────

    #[test]
    fn accepts_well_formed_key() {
        let key = ScopeKey::new("notifier:read").unwrap();
        assert_eq!(key.as_str(), "notifier:read");
        assert_eq!(key.service_segment(), "notifier");
        assert_eq!(key.capability_segment(), "read");
    }

    #[test]
    fn accepts_digits_and_underscores_in_both_segments() {
        let key = ScopeKey::new("svc_auth_2:manage_users_v1").unwrap();
        assert_eq!(key.service_segment(), "svc_auth_2");
        assert_eq!(key.capability_segment(), "manage_users_v1");
    }

    #[test]
    fn accepts_boundary_lengths() {
        // service(1) + ':' + capability(126) = 128 (the total cap, inclusive).
        let at_total = format!("a:{}", "b".repeat(SCOPE_KEY_MAX_LEN - 2));
        assert_eq!(at_total.len(), SCOPE_KEY_MAX_LEN);
        assert!(ScopeKey::new(at_total).is_ok());
        // A service segment exactly at SERVICE_KEY_MAX_LEN is accepted — the
        // per-segment bound is inclusive, matching ServiceKey.
        let at_service_max = format!("{}:read", "a".repeat(SERVICE_KEY_MAX_LEN));
        assert!(ScopeKey::new(at_service_max).is_ok());
    }

    // ─── known-bad vectors (the issue's required set) ──

    #[test]
    fn rejects_missing_colon() {
        assert_eq!(
            ScopeKey::new("notifierread"),
            Err(KeyValidationError::MalformedSegments)
        );
    }

    #[test]
    fn rejects_two_colons() {
        assert_eq!(
            ScopeKey::new("notifier:read:write"),
            Err(KeyValidationError::MalformedSegments)
        );
    }

    #[test]
    fn rejects_empty_service_segment() {
        assert_eq!(ScopeKey::new(":read"), Err(KeyValidationError::Empty));
    }

    #[test]
    fn rejects_empty_capability_segment() {
        assert_eq!(ScopeKey::new("notifier:"), Err(KeyValidationError::Empty));
    }

    #[test]
    fn rejects_both_segments_empty() {
        assert_eq!(ScopeKey::new(":"), Err(KeyValidationError::Empty));
    }

    // Uppercase, accents/UTF-8, and other forbidden characters in either
    // segment all violate the `[a-z0-9_]` charset.
    #[test]
    fn rejects_charset_violations() {
        for bad in [
            "Notifier:read",       // uppercase service
            "notifier:Read",       // uppercase capability
            "notifiér:read",       // accent / non-ASCII
            "notifier:read-write", // hyphen
            "notifier:read.write", // dot
            "notifier:read write", // space in capability
            "noti fier:read",      // space in service
        ] {
            assert_eq!(
                ScopeKey::new(bad),
                Err(KeyValidationError::InvalidCharset),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn rejects_over_max_len() {
        // 1 + ':' + (max-1) = max+1
        let over = format!("a:{}", "b".repeat(SCOPE_KEY_MAX_LEN));
        assert!(over.len() > SCOPE_KEY_MAX_LEN);
        assert_eq!(
            ScopeKey::new(over.clone()),
            Err(KeyValidationError::TooLong {
                max: SCOPE_KEY_MAX_LEN,
                actual: over.len()
            })
        );
    }

    // The `{service}` segment is bounded by SERVICE_KEY_MAX_LEN (64), not the
    // total budget: a 65-byte service segment is rejected per-segment as TooLong,
    // because no ServiceKey (also capped at 64) could ever own such a scope. This
    // is the intrinsic-consistency fix — and it makes the per-segment TooLong arm
    // genuinely reachable.
    #[test]
    fn rejects_over_long_service_segment() {
        let key = format!("{}:read", "a".repeat(SERVICE_KEY_MAX_LEN + 1));
        // The whole key is comfortably under the total budget — only the service
        // segment is too long.
        assert!(key.len() <= SCOPE_KEY_MAX_LEN);
        assert_eq!(
            ScopeKey::new(key),
            Err(KeyValidationError::TooLong {
                max: SERVICE_KEY_MAX_LEN,
                actual: SERVICE_KEY_MAX_LEN + 1,
            })
        );
    }

    // ─── contextual ownership (NOT in the constructor) ─

    #[test]
    fn is_owned_by_matches_service_segment() {
        let key = ScopeKey::new("notifier:read").unwrap();
        let owner = ServiceKey::new("notifier").unwrap();
        let other = ServiceKey::new("billing").unwrap();
        assert!(key.is_owned_by(&owner));
        assert!(!key.is_owned_by(&other));
    }

    // The constructor takes no declaring service: a key whose prefix matches no
    // particular service is still a *valid* key — ownership is judged later.
    #[test]
    fn construction_ignores_ownership() {
        assert!(ScopeKey::new("billing:read").is_ok());
    }

    // ─── serde, fail-closed ────────────────────────────

    #[test]
    fn serde_roundtrip() {
        let key = ScopeKey::new("notifier:read").unwrap();
        let json = serde_json::to_string(&key).unwrap();
        assert_eq!(json, "\"notifier:read\"");
        let back: ScopeKey = serde_json::from_str(&json).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn deserialize_rejects_malformed() {
        for bad in [
            "\"notifierread\"",
            "\"Notifier:read\"",
            "\":read\"",
            "\"a:b:c\"",
        ] {
            assert!(
                serde_json::from_str::<ScopeKey>(bad).is_err(),
                "{bad} must fail closed"
            );
        }
    }
}
