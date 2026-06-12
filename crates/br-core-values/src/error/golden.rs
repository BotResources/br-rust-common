//! Golden wire-shape tests for [`ValueError`] — the locked public contract.
//!
//! `ValueError` travels nested in other envelopes (domain errors, affordance
//! reasons), so its serialized form **is** a wire contract. The serde impl is
//! hand-rolled (see [`super::wire`]); these tests pin the EXACT JSON string for
//! every known variant so a future refactor of the hand-rolled serde cannot
//! silently drift the bytes on the wire. A drift here is a lib bug; this turns
//! "compat verified once by a human" into "compat protected in CI".

use crate::error::ValueError;

/// Assert a value serializes to EXACTLY `expected` and round-trips back equal.
fn assert_golden(value: ValueError, expected: &str) {
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(json, expected, "wire form drifted for {value:?}");
    let back: ValueError = serde_json::from_str(&json).unwrap();
    assert_eq!(back, value, "did not round-trip for {value:?}");
}

#[test]
fn golden_malformed_code() {
    assert_golden(
        ValueError::MalformedCode {
            value: "EU".into(),
            expected_len: 3,
        },
        r#"{"code":"malformed_code","value":"EU","expected_len":3}"#,
    );
}

#[test]
fn golden_unknown_currency() {
    assert_golden(
        ValueError::UnknownCurrency {
            value: "RMB".into(),
        },
        r#"{"code":"unknown_currency","value":"RMB"}"#,
    );
}

#[test]
fn golden_unknown_country() {
    assert_golden(
        ValueError::UnknownCountry { value: "UK".into() },
        r#"{"code":"unknown_country","value":"UK"}"#,
    );
}

#[test]
fn golden_localized_empty() {
    assert_golden(ValueError::LocalizedEmpty, r#"{"code":"localized_empty"}"#);
}

#[test]
fn golden_localized_primary_missing() {
    assert_golden(
        ValueError::LocalizedPrimaryMissing,
        r#"{"code":"localized_primary_missing"}"#,
    );
}

#[test]
fn golden_localized_duplicate_locale() {
    assert_golden(
        ValueError::LocalizedDuplicateLocale,
        r#"{"code":"localized_duplicate_locale"}"#,
    );
}

// `Unknown` re-emits the captured code verbatim as a param-less `{ "code": … }`.
// A code this version does not know (a real forward-compat capture) round-trips.
#[test]
fn golden_unknown_future_code() {
    assert_golden(
        ValueError::Unknown {
            code: "some_future_rule".into(),
        },
        r#"{"code":"some_future_rule"}"#,
    );
}

// ── The `Unknown` boundary: it must never be forged for a KNOWN code ─────────

// `Unknown { code }` is publicly constructible (invariant by convention: it must
// arise ONLY from deserializing an unknown code). This locks the boundary: a
// hand-built `Unknown` carrying a *known* code serializes to a param-less object
// — which is NOT a valid wire form for that code, so it fails to deserialize.
// Forging `Unknown` for a known code therefore cannot survive a wire round-trip.
#[test]
fn forged_unknown_for_a_known_code_does_not_round_trip() {
    let forged = ValueError::Unknown {
        code: "unknown_currency".into(),
    };

    // It serializes to the bare param-less shape (no `value`).
    let json = serde_json::to_string(&forged).unwrap();
    assert_eq!(json, r#"{"code":"unknown_currency"}"#);

    // But `unknown_currency` is a known code and REQUIRES `value` — so the bare
    // form fails to deserialize: the forged `Unknown` cannot come back.
    let back = serde_json::from_str::<ValueError>(&json);
    assert!(
        back.is_err(),
        "a forged Unknown for a known code must not deserialize, got {back:?}"
    );
    let msg = back.unwrap_err().to_string();
    assert!(
        msg.contains("value"),
        "the failure must name the missing required field, got {msg:?}"
    );
}
