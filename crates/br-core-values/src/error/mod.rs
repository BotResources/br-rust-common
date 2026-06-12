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
//! ## Forward-compat on the wire
//!
//! `ValueError` travels nested in other envelopes (domain errors, affordance
//! reasons). A newer producer crate may emit a `code` this (older) crate does
//! not know yet. Rather than fail the deserialization of the **whole** enclosing
//! envelope, an unrecognized `code` degrades to [`ValueError::Unknown`] carrying
//! the raw `code` string — the envelope still parses. Every code this version
//! knows stays strongly typed. `#[non_exhaustive]`: match with a wildcard arm so
//! a future rule is additive.
//!
//! The hand-rolled serde for this enum (internally tagged on `code`, with the
//! forward-compat degrade) lives in [`wire`].

mod wire;

/// Why a value-object constructor rejected its input.
///
/// Stable codes, structured params — never a rendered sentence. See the module
/// docs for the rationale and the forward-compat contract.
///
/// `Serialize`/`Deserialize` are hand-rolled in the `wire` submodule (not
/// derived) because the forward-compat [`Unknown`](Self::Unknown) variant carries
/// the `code` string itself — which collides with the internal `code` tag the
/// derive would write. The wire shape is unchanged: every variant is an object
/// tagged on `code`.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
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
    /// Forward-compat catch-all: a `code` emitted by a newer crate version that
    /// this version does not know. It is produced **only on deserialization**
    /// (never constructed by a rejecting constructor here) so an unknown future
    /// code degrades gracefully instead of failing the enclosing envelope. The
    /// raw `code` is preserved verbatim for logging / pass-through.
    #[error("{code}")]
    Unknown {
        /// The unrecognized `code` string, verbatim from the wire.
        code: String,
    },
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

    // ── Forward-compat: unknown future code degrades to Unknown ──────────────

    // Given a `code` from a newer crate version, When deserialized, Then it maps
    // to Unknown { code } — NOT a deserialization error (the envelope survives).
    #[test]
    fn unknown_future_code_degrades_to_unknown_not_an_error() {
        let wire = r#"{"code":"some_future_rule","value":"x","extra":42}"#;
        let back: ValueError = serde_json::from_str(wire).unwrap();
        assert_eq!(
            back,
            ValueError::Unknown {
                code: "some_future_rule".into()
            }
        );
    }

    // A bare unknown code (no params) also degrades, ignoring absent fields.
    #[test]
    fn unknown_future_code_without_params_degrades() {
        let wire = r#"{"code":"future_bare"}"#;
        let back: ValueError = serde_json::from_str(wire).unwrap();
        assert_eq!(
            back,
            ValueError::Unknown {
                code: "future_bare".into()
            }
        );
    }

    // A known code still deserializes to its strongly-typed variant.
    #[test]
    fn known_code_still_deserializes_to_typed_variant() {
        let wire = r#"{"code":"unknown_currency","value":"RMB"}"#;
        let back: ValueError = serde_json::from_str(wire).unwrap();
        assert_eq!(
            back,
            ValueError::UnknownCurrency {
                value: "RMB".into()
            }
        );
    }

    // Field order is irrelevant: `code` after the params still dispatches right.
    #[test]
    fn known_code_deserializes_with_fields_in_any_order() {
        let wire = r#"{"expected_len":3,"value":"EU","code":"malformed_code"}"#;
        let back: ValueError = serde_json::from_str(wire).unwrap();
        assert_eq!(
            back,
            ValueError::MalformedCode {
                value: "EU".into(),
                expected_len: 3,
            }
        );
    }

    // Round-tripping a known code is unchanged by the forward-compat path.
    #[test]
    fn known_code_roundtrip_is_unchanged() {
        let original = ValueError::MalformedCode {
            value: "EU".into(),
            expected_len: 3,
        };
        let json = serde_json::to_string(&original).unwrap();
        let back: ValueError = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }
}
