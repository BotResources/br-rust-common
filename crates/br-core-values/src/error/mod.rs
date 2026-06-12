mod wire;

#[cfg(test)]
mod golden;

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueError {
    #[error("malformed_code")]
    MalformedCode {
        value: String,
        expected_len: usize,
    },
    #[error("unknown_currency")]
    UnknownCurrency {
        value: String,
    },
    #[error("unknown_country")]
    UnknownCountry {
        value: String,
    },
    #[error("localized_empty")]
    LocalizedEmpty,
    #[error("localized_primary_missing")]
    LocalizedPrimaryMissing,
    #[error("localized_duplicate_locale")]
    LocalizedDuplicateLocale,
    #[error("{code}")]
    Unknown {
        code: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

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
