//! ISO 3166-1 alpha-2 country code, as a constructor-validated newtype.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ValueError;
use crate::iso::country_codes::COUNTRY_CODES;
use crate::iso::currency::normalize_alpha_code;

/// ISO 3166-1 alpha-2 country code (e.g. `FR`, `US`, `JP`).
///
/// Self-validating: built via [`CountryCode::new`], which trims, uppercases, and
/// validates against the complete ISO 3166-1 alpha-2 list (all 249 codes).
/// Illegal states are unrepresentable. `UK` is rejected (`GB` is correct for the
/// United Kingdom); `ZZ` is rejected.
///
/// No `Deref`: read the code via [`CountryCode::as_str`] / `AsRef<str>` /
/// `Display`. Deserialization re-runs [`CountryCode::new`] and fails closed on an
/// invalid wire value.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CountryCode(String);

impl CountryCode {
    /// Build a `CountryCode` from a raw string: trim, uppercase, require exactly
    /// 2 ASCII letters, then look the code up in the ISO 3166-1 alpha-2 list.
    ///
    /// # Errors
    /// - [`ValueError::MalformedCode`] if the trimmed input is not 2 ASCII letters.
    /// - [`ValueError::UnknownCountry`] if it is well-formed but not an ISO code.
    pub fn new(raw: &str) -> Result<Self, ValueError> {
        let upper = normalize_alpha_code(raw, 2)?;
        // O(log n) — `COUNTRY_CODES` is sorted (proven by `codes_are_sorted`).
        if COUNTRY_CODES.binary_search(&upper.as_str()).is_ok() {
            Ok(Self(upper))
        } else {
            Err(ValueError::UnknownCountry { value: upper })
        }
    }

    /// The normalized (uppercase) ISO 3166-1 alpha-2 code.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CountryCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for CountryCode {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl Serialize for CountryCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for CountryCode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        CountryCode::new(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Valid construction ───────────────────────────────────────────────────

    #[test]
    fn valid_iso_codes_are_accepted() {
        for code in &["FR", "US", "JP", "GB", "DE", "SS"] {
            assert!(CountryCode::new(code).is_ok(), "{code} should be valid");
        }
    }

    // Territories and remote islands are in the spec, not just sovereign states.
    #[test]
    fn territories_and_remote_islands_are_accepted() {
        for code in &["BM", "BV", "TF"] {
            assert!(CountryCode::new(code).is_ok(), "{code} should be valid");
        }
    }

    #[test]
    fn lowercase_is_uppercased_and_accepted() {
        assert_eq!(CountryCode::new("fr").unwrap().as_str(), "FR");
        assert_eq!(CountryCode::new("us").unwrap().as_str(), "US");
    }

    #[test]
    fn mixed_case_is_uppercased_and_accepted() {
        assert_eq!(CountryCode::new("fR").unwrap().as_str(), "FR");
    }

    #[test]
    fn whitespace_is_trimmed_before_validation() {
        assert_eq!(CountryCode::new(" FR ").unwrap().as_str(), "FR");
    }

    // ── Invalid — not in the ISO list (negative vectors) ─────────────────────

    #[test]
    fn rejects_uk_common_mistake_for_gb() {
        assert_eq!(
            CountryCode::new("UK"),
            Err(ValueError::UnknownCountry { value: "UK".into() })
        );
    }

    #[test]
    fn rejects_zz_not_an_iso_code() {
        assert_eq!(
            CountryCode::new("ZZ"),
            Err(ValueError::UnknownCountry { value: "ZZ".into() })
        );
    }

    #[test]
    fn rejects_xx_not_an_iso_code() {
        assert_eq!(
            CountryCode::new("XX"),
            Err(ValueError::UnknownCountry { value: "XX".into() })
        );
    }

    // ── Invalid — malformed (negative vectors) ───────────────────────────────

    #[test]
    fn rejects_empty_string() {
        assert!(matches!(
            CountryCode::new(""),
            Err(ValueError::MalformedCode {
                expected_len: 2,
                ..
            })
        ));
    }

    #[test]
    fn rejects_single_character() {
        assert!(matches!(
            CountryCode::new("F"),
            Err(ValueError::MalformedCode { .. })
        ));
    }

    #[test]
    fn rejects_three_or_more_characters() {
        assert!(matches!(
            CountryCode::new("FRA"),
            Err(ValueError::MalformedCode { .. })
        ));
        assert!(matches!(
            CountryCode::new("FRANCE"),
            Err(ValueError::MalformedCode { .. })
        ));
    }

    #[test]
    fn rejects_digits() {
        assert!(matches!(
            CountryCode::new("F1"),
            Err(ValueError::MalformedCode { .. })
        ));
        assert!(matches!(
            CountryCode::new("12"),
            Err(ValueError::MalformedCode { .. })
        ));
    }

    #[test]
    fn rejects_special_characters() {
        assert!(matches!(
            CountryCode::new("F-"),
            Err(ValueError::MalformedCode { .. })
        ));
    }

    // ── Display, access, serde ───────────────────────────────────────────────

    #[test]
    fn display_and_as_ref_produce_uppercase_code() {
        let c = CountryCode::new("us").unwrap();
        assert_eq!(c.to_string(), "US");
        let s: &str = c.as_ref();
        assert_eq!(s, "US");
    }

    #[test]
    fn serialized_form_is_plain_json_string() {
        let json = serde_json::to_string(&CountryCode::new("FR").unwrap()).unwrap();
        assert_eq!(json, "\"FR\"");
    }

    #[test]
    fn serde_roundtrip() {
        let original = CountryCode::new("JP").unwrap();
        let json = serde_json::to_string(&original).unwrap();
        let back: CountryCode = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    // Given an invalid wire value, When deserialized, Then it fails closed.
    #[test]
    fn deserialize_rejects_unknown_code() {
        assert!(serde_json::from_str::<CountryCode>("\"ZZ\"").is_err());
    }

    #[test]
    fn deserialize_rejects_malformed_code() {
        assert!(serde_json::from_str::<CountryCode>("\"FRA\"").is_err());
    }

    // ── Equality ─────────────────────────────────────────────────────────────

    #[test]
    fn equality_is_by_normalized_code() {
        assert_eq!(
            CountryCode::new("fr").unwrap(),
            CountryCode::new("FR").unwrap()
        );
        assert_ne!(
            CountryCode::new("FR").unwrap(),
            CountryCode::new("US").unwrap()
        );
    }
}
