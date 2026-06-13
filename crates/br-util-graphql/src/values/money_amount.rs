use async_graphql::{InputValueError, InputValueResult, Scalar, ScalarType, Value};

use crate::values::error::GqlValueError;

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
    fn parse(value: Value) -> InputValueResult<Self> {
        match value {
            Value::String(s) => s.parse::<i64>().map(MoneyAmount).map_err(|_| {
                InputValueError::custom(
                    GqlValueError::MoneyOutOfRange { amount: s.clone() }.reason_code(),
                )
            }),
            other => Err(InputValueError::expected_type(other)),
        }
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.to_string())
    }
}

#[cfg(test)]
mod tests {
    use async_graphql::Pos;

    use super::*;

    fn parse_error_message(value: Value) -> String {
        MoneyAmount::parse(value)
            .unwrap_err()
            .into_server_error(Pos::default())
            .message
    }

    #[test]
    fn serializes_to_decimal_string() {
        assert_eq!(
            MoneyAmount(4250).to_value(),
            Value::String("4250".to_owned())
        );
    }

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

    #[test]
    fn parses_decimal_string_to_i64() {
        let parsed = MoneyAmount::parse(Value::String("123456789012".to_owned())).unwrap();
        assert_eq!(parsed, MoneyAmount(123_456_789_012));
    }

    #[test]
    fn refuses_string_beyond_i64_with_money_out_of_range() {
        let beyond = "99999999999999999999";
        let msg = parse_error_message(Value::String(beyond.to_owned()));
        assert!(msg.contains("MONEY_OUT_OF_RANGE"), "got: {msg}");
    }

    #[test]
    fn refuses_non_numeric_string_with_money_out_of_range() {
        for bad in ["12.50", "abc", ""] {
            let msg = parse_error_message(Value::String(bad.to_owned()));
            assert!(msg.contains("MONEY_OUT_OF_RANGE"), "bad={bad} got: {msg}");
        }
    }

    #[test]
    fn refuses_non_string_value() {
        assert!(MoneyAmount::parse(Value::Number(4250.into())).is_err());
    }
}
