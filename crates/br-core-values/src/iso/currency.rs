//! ISO 4217 alphabetic currency code, as a constructor-validated newtype.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ValueError;
use crate::iso::currency_codes::CURRENCY_CODES;

/// ISO 4217 alphabetic currency code (e.g. `EUR`, `USD`, `JPY`).
///
/// Self-validating: built via [`Currency::new`], which trims, uppercases, and
/// validates against the complete ISO 4217 active currency list. Illegal states
/// are unrepresentable — there is no way to hold a `Currency` that is not a real
/// ISO code. `RMB` is rejected (`CNY` is correct for the Chinese Yuan); `ZZZ` is
/// rejected.
///
/// No `Deref`: read the code via [`Currency::as_str`] / `AsRef<str>` /
/// `Display`. Deserialization re-runs [`Currency::new`] and fails closed on an
/// invalid wire value — serde is a constructor path, not a backdoor.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Currency(String);

impl Currency {
    /// Build a `Currency` from a raw string: trim, uppercase, require exactly 3
    /// ASCII letters, then look the code up in the ISO 4217 active list.
    ///
    /// # Errors
    /// - [`ValueError::MalformedCode`] if the trimmed input is not 3 ASCII letters.
    /// - [`ValueError::UnknownCurrency`] if it is well-formed but not an ISO code.
    pub fn new(raw: &str) -> Result<Self, ValueError> {
        let upper = normalize_alpha_code(raw, 3)?;
        // O(log n) — `CURRENCY_CODES` is sorted (proven by `codes_are_sorted`).
        if CURRENCY_CODES.binary_search(&upper.as_str()).is_ok() {
            Ok(Self(upper))
        } else {
            Err(ValueError::UnknownCurrency { value: upper })
        }
    }

    /// The normalized (uppercase) ISO 4217 code.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Trim, uppercase, and require exactly `len` ASCII letters. Shared by the ISO
/// code newtypes; returns the normalized form or [`ValueError::MalformedCode`].
pub(crate) fn normalize_alpha_code(raw: &str, len: usize) -> Result<String, ValueError> {
    let upper = raw.trim().to_uppercase();
    if upper.len() == len && upper.bytes().all(|b| b.is_ascii_alphabetic()) {
        Ok(upper)
    } else {
        Err(ValueError::MalformedCode {
            value: raw.to_string(),
            expected_len: len,
        })
    }
}

impl fmt::Display for Currency {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Currency {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for Currency {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Currency {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Currency::new(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Valid construction ───────────────────────────────────────────────────

    // Given well-known codes, When constructed, Then accepted and normalized.
    #[test]
    fn well_known_currencies_are_accepted() {
        for code in &["EUR", "USD", "JPY", "GBP", "CNY", "BMD", "XPF"] {
            assert!(Currency::new(code).is_ok(), "{code} should be valid");
        }
    }

    #[test]
    fn lowercase_is_uppercased_and_accepted() {
        assert_eq!(Currency::new("eur").unwrap().as_str(), "EUR");
    }

    #[test]
    fn mixed_case_is_uppercased_and_accepted() {
        assert_eq!(Currency::new("Eur").unwrap().as_str(), "EUR");
    }

    #[test]
    fn whitespace_is_trimmed_before_validation() {
        assert_eq!(Currency::new(" EUR ").unwrap().as_str(), "EUR");
    }

    // ── Invalid — not in the ISO list (negative vectors) ─────────────────────

    // Given a common mistake (RMB for CNY), When constructed, Then UnknownCurrency.
    #[test]
    fn rejects_rmb_common_mistake_for_cny() {
        assert_eq!(
            Currency::new("RMB"),
            Err(ValueError::UnknownCurrency {
                value: "RMB".into()
            })
        );
    }

    #[test]
    fn rejects_zzz_not_an_iso_code() {
        assert_eq!(
            Currency::new("ZZZ"),
            Err(ValueError::UnknownCurrency {
                value: "ZZZ".into()
            })
        );
    }

    // ── Invalid — malformed (negative vectors) ───────────────────────────────

    #[test]
    fn rejects_empty_string() {
        assert!(matches!(
            Currency::new(""),
            Err(ValueError::MalformedCode {
                expected_len: 3,
                ..
            })
        ));
    }

    #[test]
    fn rejects_two_letter_code() {
        assert!(matches!(
            Currency::new("EU"),
            Err(ValueError::MalformedCode {
                expected_len: 3,
                ..
            })
        ));
    }

    #[test]
    fn rejects_four_or_more_letters() {
        assert!(matches!(
            Currency::new("EURO"),
            Err(ValueError::MalformedCode { .. })
        ));
    }

    #[test]
    fn rejects_codes_with_digits() {
        assert!(matches!(
            Currency::new("EU1"),
            Err(ValueError::MalformedCode { .. })
        ));
        assert!(matches!(
            Currency::new("123"),
            Err(ValueError::MalformedCode { .. })
        ));
    }

    #[test]
    fn malformed_error_keeps_the_raw_input() {
        assert_eq!(
            Currency::new("eu"),
            Err(ValueError::MalformedCode {
                value: "eu".into(),
                expected_len: 3,
            })
        );
    }

    // ── Display, access, serde ───────────────────────────────────────────────

    #[test]
    fn display_and_as_ref_produce_uppercase_code() {
        let c = Currency::new("EUR").unwrap();
        assert_eq!(c.to_string(), "EUR");
        let s: &str = c.as_ref();
        assert_eq!(s, "EUR");
    }

    #[test]
    fn serialized_form_is_plain_json_string() {
        let json = serde_json::to_string(&Currency::new("JPY").unwrap()).unwrap();
        assert_eq!(json, "\"JPY\"");
    }

    #[test]
    fn serde_roundtrip() {
        let original = Currency::new("JPY").unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let back: Currency = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    // Given an invalid wire value, When deserialized, Then it fails closed.
    #[test]
    fn deserialize_rejects_unknown_code() {
        assert!(serde_json::from_str::<Currency>("\"RMB\"").is_err());
    }

    #[test]
    fn deserialize_rejects_malformed_code() {
        assert!(serde_json::from_str::<Currency>("\"EU\"").is_err());
    }

    // ── Equality ─────────────────────────────────────────────────────────────

    #[test]
    fn equality_is_by_normalized_code() {
        assert_eq!(Currency::new("eur").unwrap(), Currency::new("EUR").unwrap());
        assert_ne!(Currency::new("EUR").unwrap(), Currency::new("USD").unwrap());
    }
}
