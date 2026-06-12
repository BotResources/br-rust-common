//! [`GqlValueError`] — why a fallible VO ↔ GraphQL conversion was refused.
//!
//! These wrappers **fail with a typed code, they never silently coerce** — the
//! exact inverse of the Hanshow seed (which coerced an unknown locale to the
//! default, accepted missing primary content, and truncated `Money` i64→i32; we
//! carry the full `i64` as a decimal string instead — see [`GqlMoney`]).
//! Each refusal is a stable code, never UI prose (codes-not-language); it maps
//! cleanly to [`ErrorCode::BadUserInput`] / [`ErrorCode::Internal`] at the edge.
//!
//! [`GqlMoney`]: crate::values::GqlMoney
//!
//! [`ErrorCode::BadUserInput`]: crate::ErrorCode::BadUserInput
//! [`ErrorCode::Internal`]: crate::ErrorCode::Internal

use br_core_values::ValueError;

use crate::error::EdgeError;

/// A fallible-conversion rejection. Stable codes, structured params, never a
/// sentence. `#[non_exhaustive]` — match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GqlValueError {
    /// A locale string sent by the client did not match any variant of the
    /// service's `Locale` enum. We refuse — never coerce to a default locale.
    /// Carries the offending input.
    LocaleUnknown {
        /// The locale string as supplied by the client.
        value: String,
    },
    /// An inbound `MoneyAmount` decimal string did not parse into an `i64` —
    /// non-numeric, or beyond the `i64` range. We refuse — never truncate or
    /// wrap. (The full `i64` range is carried as a decimal string, so this is the
    /// parse/overflow boundary, not an `Int` ceiling.) Carries the raw input.
    MoneyOutOfRange {
        /// The decimal-string amount supplied by the client that failed to parse.
        amount: String,
    },
    /// A localized input carried no entry for its declared primary locale (or
    /// no entries at all). We refuse — never accept missing primary content.
    PrimaryContentMissing,
    /// A wrapped value-object rejection that is not one of the named conversion
    /// cases above (e.g. an unknown ISO currency on a money input). The
    /// underlying [`ValueError`]'s own stable code is surfaced as the reason —
    /// never re-labelled as one of the three named conversion codes, never
    /// coerced away.
    ValueRejected {
        /// The value-object rejection, carrying its own stable code.
        source: ValueError,
    },
}

impl GqlValueError {
    /// The stable wire code for this rejection (codes-not-language). For a
    /// wrapped [`ValueError`] the code is the value object's own
    /// (`unknown_currency`, …), kept as-is.
    pub fn reason_code(&self) -> String {
        match self {
            GqlValueError::LocaleUnknown { .. } => "LOCALE_UNKNOWN".to_owned(),
            GqlValueError::MoneyOutOfRange { .. } => "MONEY_OUT_OF_RANGE".to_owned(),
            GqlValueError::PrimaryContentMissing => "PRIMARY_CONTENT_MISSING".to_owned(),
            // The value object already speaks in codes — pass its code through.
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
    /// Every conversion rejection is a bad-input error at the edge, carrying the
    /// precise `reason_code` (+ the offending value as a param).
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

    // codes-not-language: each variant's reason code is a stable UPPER_SNAKE
    // key, exactly the three the issue specifies.
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

    // A wrapped ValueError passes its own stable code through, never re-labelled.
    #[test]
    fn wrapped_value_error_surfaces_its_own_code() {
        let err = GqlValueError::ValueRejected {
            source: ValueError::UnknownCurrency {
                value: "RMB".into(),
            },
        };
        assert_eq!(err.reason_code(), "unknown_currency");
    }

    // A conversion rejection becomes a BAD_USER_INPUT edge error carrying the
    // precise reason + the offending value as a param.
    #[test]
    fn maps_to_bad_user_input_with_reason_and_param() {
        let edge: EdgeError = GqlValueError::LocaleUnknown { value: "xx".into() }.into();
        assert_eq!(edge.code(), ErrorCode::BadUserInput);
        assert_eq!(edge.reason_code(), Some("LOCALE_UNKNOWN"));
        assert_eq!(edge.params().get("value").map(String::as_str), Some("xx"));
    }
}
