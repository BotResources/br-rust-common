//! A monetary amount: a minor-unit `i64` paired with a validated [`Currency`].

use serde::{Deserialize, Serialize};

use crate::iso::currency::Currency;

/// A monetary amount in minor units, paired with its currency.
///
/// `amount` is stored as `i64` in the currency's minor unit (centimes for EUR,
/// cents for USD, yen for JPY — JPY has no minor unit, so the amount *is* yen).
/// Negative values represent credits or refunds.
///
/// `Money` has **no validated invariant of its own beyond the currency**: any
/// `i64` is a legal amount, and the [`Currency`] field is self-validating (it
/// cannot be constructed, nor deserialized, as a non-ISO code). The fields are
/// therefore `pub` and serde is derived — there is no constructor to bypass,
/// because a `Money` carrying an invalid currency is already unrepresentable.
///
/// There are intentionally **no arithmetic methods** on this type. Monetary
/// arithmetic (rounding, cross-currency conversion, allocation) is domain
/// policy, not a universal value-object concern — it belongs in the consuming
/// domain, not in this crate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    /// The amount in the currency's minor unit. May be negative (credit/refund).
    pub amount: i64,
    /// The currency the amount is denominated in.
    pub currency: Currency,
}

impl Money {
    /// Build a `Money` from a minor-unit amount and a currency. Total — any
    /// `i64` amount is legal and the currency is already validated.
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

    // ── Construction ─────────────────────────────────────────────────────────

    #[test]
    fn stores_positive_amount_in_minor_units() {
        assert_eq!(Money::new(4250, eur()).amount, 4250);
    }

    #[test]
    fn stores_zero_amount() {
        assert_eq!(Money::new(0, eur()).amount, 0);
    }

    // Given a refund, When modeled, Then a negative amount is preserved.
    #[test]
    fn stores_negative_amount_for_credits_and_refunds() {
        assert_eq!(Money::new(-1500, eur()).amount, -1500);
    }

    #[test]
    fn works_with_zero_decimal_currency() {
        let jpy = Currency::new("JPY").unwrap();
        assert_eq!(Money::new(1000, jpy).amount, 1000);
    }

    // ── Serde ────────────────────────────────────────────────────────────────

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

    // The currency field is self-validating even through Money's derived deser.
    #[test]
    fn deserialize_rejects_invalid_currency() {
        let bad = r#"{"amount":100,"currency":"RMB"}"#;
        assert!(serde_json::from_str::<Money>(bad).is_err());
    }

    // ── Equality ─────────────────────────────────────────────────────────────

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
