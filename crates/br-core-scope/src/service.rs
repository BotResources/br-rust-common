use std::fmt;

use serde::de::{Deserializer, Error as _};
use serde::{Deserialize, Serialize, Serializer};

use crate::error::KeyValidationError;

pub const SERVICE_KEY_MAX_LEN: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ServiceKey(String);

impl ServiceKey {
    pub fn new(value: impl Into<String>) -> Result<Self, KeyValidationError> {
        let value = value.into();
        validate_segment(&value, SERVICE_KEY_MAX_LEN)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

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

impl<'de> Deserialize<'de> for ServiceKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Self::new(raw).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_service_key() {
        let key = ServiceKey::new("notifier").unwrap();
        assert_eq!(key.as_str(), "notifier");
    }

    #[test]
    fn accepts_digits_and_underscores() {
        assert!(ServiceKey::new("svc_auth_2").is_ok());
    }

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

    #[test]
    fn deserialize_rejects_malformed() {
        let err = serde_json::from_str::<ServiceKey>("\"Notifier\"");
        assert!(err.is_err());
    }
}
