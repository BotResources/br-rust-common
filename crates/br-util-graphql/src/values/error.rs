use br_core_values::ValueError;

use crate::error::EdgeError;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GqlValueError {
    LocaleUnknown { value: String },
    MoneyOutOfRange { amount: String },
    PrimaryContentMissing,
    ValueRejected { source: ValueError },
}

impl GqlValueError {
    pub fn reason_code(&self) -> String {
        match self {
            GqlValueError::LocaleUnknown { .. } => "LOCALE_UNKNOWN".to_owned(),
            GqlValueError::MoneyOutOfRange { .. } => "MONEY_OUT_OF_RANGE".to_owned(),
            GqlValueError::PrimaryContentMissing => "PRIMARY_CONTENT_MISSING".to_owned(),
            GqlValueError::ValueRejected { source } => source.to_string(),
        }
    }
}

impl std::fmt::Display for GqlValueError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.reason_code())
    }
}

impl std::error::Error for GqlValueError {}

impl From<GqlValueError> for EdgeError {
    fn from(error: GqlValueError) -> Self {
        let edge = EdgeError::bad_user_input().with_reason(error.reason_code());
        match &error {
            GqlValueError::LocaleUnknown { value } => edge.with_param("value", value),
            GqlValueError::MoneyOutOfRange { amount } => edge.with_param("amount", amount),
            GqlValueError::PrimaryContentMissing => edge,
            GqlValueError::ValueRejected { source } => edge.with_param("value", source.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    #[test]
    fn reason_codes_are_the_three_typed_keys() {
        assert_eq!(
            GqlValueError::LocaleUnknown { value: "xx".into() }.reason_code(),
            "LOCALE_UNKNOWN"
        );
        assert_eq!(
            GqlValueError::MoneyOutOfRange {
                amount: "99999999999999999999".into()
            }
            .reason_code(),
            "MONEY_OUT_OF_RANGE"
        );
        assert_eq!(
            GqlValueError::PrimaryContentMissing.reason_code(),
            "PRIMARY_CONTENT_MISSING"
        );
    }

    #[test]
    fn wrapped_value_error_surfaces_its_own_code() {
        let err = GqlValueError::ValueRejected {
            source: ValueError::UnknownCurrency {
                value: "RMB".into(),
            },
        };
        assert_eq!(err.reason_code(), "unknown_currency");
    }

    #[test]
    fn maps_to_bad_user_input_with_reason_and_param() {
        let edge: EdgeError = GqlValueError::LocaleUnknown { value: "xx".into() }.into();
        assert_eq!(edge.code(), ErrorCode::BadUserInput);
        assert_eq!(edge.reason_code(), Some("LOCALE_UNKNOWN"));
        assert_eq!(edge.params().get("value").map(String::as_str), Some("xx"));
    }
}
