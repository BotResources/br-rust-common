use serde::{Deserialize, Serialize};

use crate::iso::currency::Currency;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    pub amount: i64,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: i64, currency: Currency) -> Self {
        Self { amount, currency }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur() -> Currency {
        Currency::new("EUR").unwrap()
    }

    #[test]
    fn stores_positive_amount_in_minor_units() {
        assert_eq!(Money::new(4250, eur()).amount, 4250);
    }

    #[test]
    fn stores_zero_amount() {
        assert_eq!(Money::new(0, eur()).amount, 0);
    }

    #[test]
    fn stores_negative_amount_for_credits_and_refunds() {
        assert_eq!(Money::new(-1500, eur()).amount, -1500);
    }

    #[test]
    fn works_with_zero_decimal_currency() {
        let jpy = Currency::new("JPY").unwrap();
        assert_eq!(Money::new(1000, jpy).amount, 1000);
    }

    #[test]
    fn serialized_json_shape() {
        let json = serde_json::to_value(Money::new(4250, eur())).unwrap();
        assert_eq!(json["amount"], 4250);
        assert_eq!(json["currency"], "EUR");
    }

    #[test]
    fn serde_roundtrip() {
        let original = Money::new(4250, eur());
        let json = serde_json::to_string(&original).unwrap();
        let back: Money = serde_json::from_str(&json).unwrap();
        assert_eq!(original, back);
    }

    #[test]
    fn negative_amount_survives_roundtrip() {
        let original = Money::new(-500, Currency::new("USD").unwrap());
        let json = serde_json::to_string(&original).unwrap();
        let back: Money = serde_json::from_str(&json).unwrap();
        assert_eq!(back.amount, -500);
    }

    #[test]
    fn deserialize_rejects_invalid_currency() {
        let bad = r#"{"amount":100,"currency":"RMB"}"#;
        assert!(serde_json::from_str::<Money>(bad).is_err());
    }

    #[test]
    fn equality_is_by_amount_and_currency() {
        assert_eq!(Money::new(1000, eur()), Money::new(1000, eur()));
        assert_ne!(
            Money::new(1000, eur()),
            Money::new(1000, Currency::new("USD").unwrap())
        );
        assert_ne!(Money::new(1000, eur()), Money::new(2000, eur()));
    }
}
