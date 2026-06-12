//! [`MoneyAmount`] — a typed GraphQL **scalar** carrying a full `i64` minor-unit
//! amount on the wire as a **decimal string** (e.g. `"123456789012"`).
//!
//! Why a string, not the built-in `Int`? GraphQL's `Int` is 32-bit by spec, so
//! it caps a money amount at ≈ 21.5 M for a 2-decimal currency — far too small
//! for B2B. The naive fix (a numeric scalar over `i64`) is **not precision-safe
//! either**: JSON numbers are IEEE-754 doubles, which lose integer precision
//! above 2⁵³, so a large `i64` would silently corrupt in any JS/JSON client. The
//! standard, JS-safe representation for large-integer money is therefore a
//! **decimal string** (the GitHub / Stripe convention). [`MoneyAmount`] is that:
//! it serializes the full `i64` to a decimal string and parses one back, with no
//! ceiling within `i64` and no truncation in either direction.
//!
//! This scalar is **infallible on output** (every `i64` has a decimal form). On
//! input the parse boundary lives one layer up, in
//! [`GqlMoneyInput`](super::money::GqlMoneyInput)'s conversion, which raises the
//! typed [`MoneyOutOfRange`](super::error::GqlValueError::MoneyOutOfRange) code on
//! a non-numeric or overflowing string — keeping the structured codes-not-language
//! boundary the rest of the `values` module is built on.

use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};

use crate::values::error::GqlValueError;

/// A monetary amount in a currency's minor unit, carried on the GraphQL wire as a
/// **decimal string** so the full `i64` range survives precision-safely (see the
/// module note). Wraps the same `i64` as [`br_core_values::Money::amount`], with
/// no ceiling and no truncation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MoneyAmount(pub i64);

impl From<i64> for MoneyAmount {
    fn from(amount: i64) -> Self {
        Self(amount)
    }
}

impl From<MoneyAmount> for i64 {
    fn from(amount: MoneyAmount) -> Self {
        amount.0
    }
}

#[Scalar(name = "MoneyAmount")]
impl ScalarType for MoneyAmount {
    /// Parse a decimal-string amount into the full `i64`. A non-numeric or
    /// out-of-`i64`-range string is **refused** (never truncated). The GraphQL
    /// layer reports it as an input-value error; the typed
    /// [`MoneyOutOfRange`](super::error::GqlValueError::MoneyOutOfRange) code is
    /// raised at the [`GqlMoneyInput`](super::money::GqlMoneyInput) boundary.
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => s.parse::<i64>().map(MoneyAmount).map_err(|_| {
                // The code is defined once, on the typed variant — never inlined.
                InputValueError::custom(
                    GqlValueError::MoneyOutOfRange { amount: s.clone() }.reason_code(),
                )
            }),
            other => Err(InputValueError::expected_type(other)),
        }
    }

    /// Serialize the full `i64` amount as a decimal string — exact, uncapped,
    /// never truncated.
    fn to_value(&self) -> Value {
        Value::String(self.0.to_string())
    }
}

#[cfg(test)]
mod tests {
    use async_graphql::Pos;

    use super::*;

    // The MONEY_OUT_OF_RANGE key the scalar's parse-failure carries, surfaced
    // through the wire error message.
    fn parse_error_message(value: Value) -> String {
        MoneyAmount::parse(value)
            .unwrap_err()
            .into_server_error(Pos::default())
            .message
    }

    // ── Output: exact, uncapped serialization ─────────────────────────────────

    // Given an i64 amount, When serialized, Then it is a decimal string.
    #[test]
    fn serializes_to_decimal_string() {
        assert_eq!(
            MoneyAmount(4250).to_value(),
            Value::String("4250".to_owned())
        );
    }

    // Given an amount far above i32::MAX, When serialized, Then the full value
    // survives as a decimal string — no truncation, no ceiling.
    #[test]
    fn serializes_large_i64_without_truncation() {
        let big = i64::from(i32::MAX) * 1_000_000;
        assert_eq!(MoneyAmount(big).to_value(), Value::String(big.to_string()));
    }

    #[test]
    fn serializes_negative_amount() {
        assert_eq!(
            MoneyAmount(-1500).to_value(),
            Value::String("-1500".to_owned())
        );
    }

    // ── Input: parse the decimal string back to i64 ───────────────────────────

    // Given a decimal string, When parsed, Then it round-trips to the same i64.
    #[test]
    fn parses_decimal_string_to_i64() {
        let parsed = MoneyAmount::parse(Value::String("123456789012".to_owned())).unwrap();
        assert_eq!(parsed, MoneyAmount(123_456_789_012));
    }

    // Given a string above i64::MAX, When parsed, Then it is refused with the
    // MONEY_OUT_OF_RANGE key — never truncated or wrapped.
    #[test]
    fn refuses_string_beyond_i64_with_money_out_of_range() {
        let beyond = "99999999999999999999"; // > i64::MAX
        let msg = parse_error_message(Value::String(beyond.to_owned()));
        assert!(msg.contains("MONEY_OUT_OF_RANGE"), "got: {msg}");
    }

    // Given a non-numeric string, When parsed, Then it is refused with the
    // MONEY_OUT_OF_RANGE key.
    #[test]
    fn refuses_non_numeric_string_with_money_out_of_range() {
        for bad in ["12.50", "abc", ""] {
            let msg = parse_error_message(Value::String(bad.to_owned()));
            assert!(msg.contains("MONEY_OUT_OF_RANGE"), "bad={bad} got: {msg}");
        }
    }

    // Given a non-string GraphQL value (e.g. a number), Then it is refused — the
    // wire form is a string by contract.
    #[test]
    fn refuses_non_string_value() {
        assert!(MoneyAmount::parse(Value::Number(4250.into())).is_err());
    }
}
