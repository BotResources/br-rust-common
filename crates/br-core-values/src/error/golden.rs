use crate::error::ValueError;

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

#[test]
fn golden_unknown_future_code() {
    assert_golden(
        ValueError::Unknown {
            code: "some_future_rule".into(),
        },
        r#"{"code":"some_future_rule"}"#,
    );
}

#[test]
fn forged_unknown_for_a_known_code_does_not_round_trip() {
    let forged = ValueError::Unknown {
        code: "unknown_currency".into(),
    };

    let json = serde_json::to_string(&forged).unwrap();
    assert_eq!(json, r#"{"code":"unknown_currency"}"#);

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
