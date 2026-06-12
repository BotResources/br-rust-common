//! The crate's own error type for value-object construction.
//!
//! One enum, [`ValueError`], for every constructor in the crate. Per the
//! codes-not-language rule the `#[error("…")]` strings are **stable codes**
//! (`unknown_currency`, `localized_empty`, …), never UI prose: the human text
//! and its i18n live at the edge. The variants carry structured params (the
//! offending value, the max/actual length) so a caller — or a test — can branch
//! on the exact rule that was broken, and so the edge can render a precise
//! message per locale.
//!
//! It (de)serializes (internally tagged on `code`) so a rejection reason can
//! travel on the wire — e.g. nested in a domain error or an affordance reason —
//! without re-stringifying.
//!
//! `#[non_exhaustive]`: match with a wildcard arm so a future rule is additive.

use serde::{Deserialize, Serialize};

/// Why a value-object constructor rejected its input.
///
/// Stable codes, structured params — never a rendered sentence. See the module
/// docs for the rationale.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
#[non_exhaustive]
pub enum ValueError {
    /// A currency/country code was not exactly the required number of ASCII
    /// letters (3 for ISO 4217, 2 for ISO 3166-1 alpha-2).
    #[error("malformed_code")]
    MalformedCode {
        /// The offending input, as supplied (before normalization).
        value: String,
        /// The number of ASCII letters the code must have.
        expected_len: usize,
    },
    /// A syntactically valid code that is not in the ISO 4217 active list.
    #[error("unknown_currency")]
    UnknownCurrency {
        /// The normalized (trimmed, uppercased) code that was looked up.
        value: String,
    },
    /// A syntactically valid code that is not in the ISO 3166-1 alpha-2 list.
    #[error("unknown_country")]
    UnknownCountry {
        /// The normalized (trimmed, uppercased) code that was looked up.
        value: String,
    },
    /// A localized value was built with no entries at all. Every localized value
    /// must carry at least its primary entry.
    #[error("localized_empty")]
    LocalizedEmpty,
    /// A localized value's `entries` did not contain an entry for its declared
    /// `primary` locale.
    #[error("localized_primary_missing")]
    LocalizedPrimaryMissing,
    /// The same locale appeared more than once in a localized value's `entries`.
    #[error("localized_duplicate_locale")]
    LocalizedDuplicateLocale,
}

#[cfg(test)]
mod tests {
    use super::*;

    // codes-not-language: the Display string is a stable code, never a sentence.
    #[test]
    fn codes_are_stable_keys() {
        assert_eq!(
            ValueError::MalformedCode {
                value: "EU".into(),
                expected_len: 3
            }
            .to_string(),
            "malformed_code"
        );
        assert_eq!(
            ValueError::UnknownCurrency {
                value: "RMB".into()
            }
            .to_string(),
            "unknown_currency"
        );
        assert_eq!(
            ValueError::UnknownCountry { value: "UK".into() }.to_string(),
            "unknown_country"
        );
        assert_eq!(ValueError::LocalizedEmpty.to_string(), "localized_empty");
        assert_eq!(
            ValueError::LocalizedPrimaryMissing.to_string(),
            "localized_primary_missing"
        );
        assert_eq!(
            ValueError::LocalizedDuplicateLocale.to_string(),
            "localized_duplicate_locale"
        );
    }

    // The error can be a wire payload (nested in a rejection); lock its tagged shape.
    #[test]
    fn wire_shape_is_tagged_on_code() {
        let err = ValueError::MalformedCode {
            value: "EU".into(),
            expected_len: 3,
        };
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "malformed_code");
        assert_eq!(json["value"], "EU");
        assert_eq!(json["expected_len"], 3);
    }

    #[test]
    fn serde_roundtrip_all_variants() {
        let variants = [
            ValueError::MalformedCode {
                value: "EU".into(),
                expected_len: 3,
            },
            ValueError::UnknownCurrency {
                value: "RMB".into(),
            },
            ValueError::UnknownCountry { value: "UK".into() },
            ValueError::LocalizedEmpty,
            ValueError::LocalizedPrimaryMissing,
            ValueError::LocalizedDuplicateLocale,
        ];
        for v in variants {
            let json = serde_json::to_string(&v).unwrap();
            let back: ValueError = serde_json::from_str(&json).unwrap();
            assert_eq!(v, back);
        }
    }
}
