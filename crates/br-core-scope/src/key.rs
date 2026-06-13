use std::fmt;

use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, Serialize, Serializer};

use crate::error::KeyValidationError;
use crate::service::{SERVICE_KEY_MAX_LEN, ServiceKey, validate_segment};

pub const SCOPE_KEY_MAX_LEN: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScopeKey {
    raw: String,
    colon: usize,
}

impl ScopeKey {
    pub fn new(value: impl Into<String>) -> Result<Self, KeyValidationError> {
        let value = value.into();
        if value.len() > SCOPE_KEY_MAX_LEN {
            return Err(KeyValidationError::TooLong {
                max: SCOPE_KEY_MAX_LEN,
                actual: value.len(),
            });
        }
        let mut parts = value.split(':');
        let (Some(service), Some(capability), None) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(KeyValidationError::MalformedSegments);
        };
        validate_segment(service, SERVICE_KEY_MAX_LEN)?;
        validate_segment(capability, SCOPE_KEY_MAX_LEN)?;
        let colon = service.len();
        Ok(Self { raw: value, colon })
    }

    pub fn as_str(&self) -> &str {
        &self.raw
    }

    pub fn service_segment(&self) -> &str {
        &self.raw[..self.colon]
    }

    pub fn capability_segment(&self) -> &str {
        &self.raw[self.colon + 1..]
    }

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

impl<'de> Deserialize<'de> for ScopeKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let at_total = format!("a:{}", "b".repeat(SCOPE_KEY_MAX_LEN - 2));
        assert_eq!(at_total.len(), SCOPE_KEY_MAX_LEN);
        assert!(ScopeKey::new(at_total).is_ok());
        let at_service_max = format!("{}:read", "a".repeat(SERVICE_KEY_MAX_LEN));
        assert!(ScopeKey::new(at_service_max).is_ok());
    }

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

    #[test]
    fn rejects_charset_violations() {
        for bad in [
            "Notifier:read",
            "notifier:Read",
            "notifiér:read",
            "notifier:read-write",
            "notifier:read.write",
            "notifier:read write",
            "noti fier:read",
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

    #[test]
    fn rejects_over_long_service_segment() {
        let key = format!("{}:read", "a".repeat(SERVICE_KEY_MAX_LEN + 1));
        assert!(key.len() <= SCOPE_KEY_MAX_LEN);
        assert_eq!(
            ScopeKey::new(key),
            Err(KeyValidationError::TooLong {
                max: SERVICE_KEY_MAX_LEN,
                actual: SERVICE_KEY_MAX_LEN + 1,
            })
        );
    }

    #[test]
    fn is_owned_by_matches_service_segment() {
        let key = ScopeKey::new("notifier:read").unwrap();
        let owner = ServiceKey::new("notifier").unwrap();
        let other = ServiceKey::new("billing").unwrap();
        assert!(key.is_owned_by(&owner));
        assert!(!key.is_owned_by(&other));
    }

    #[test]
    fn construction_ignores_ownership() {
        assert!(ScopeKey::new("billing:read").is_ok());
    }

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
