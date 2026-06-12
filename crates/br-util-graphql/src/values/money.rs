//! [`GqlMoney`] / [`GqlMoneyInput`] — GraphQL wrappers over
//! [`br_core_values::Money`] that carry the **full `i64`** minor-unit amount
//! end-to-end, never truncated.
//!
//! The anti-model (Hanshow) truncated the minor-unit amount `i64` → `i32` to fit
//! the GraphQL `Int` scalar, silently corrupting large amounts. The fix is **not**
//! to keep a smaller ceiling: it is to stop using `Int` for money. Both wrappers
//! carry the amount through the [`MoneyAmount`] scalar — a decimal **string** on
//! the wire (e.g. `"123456789012"`), JS/JSON-precision-safe for the whole `i64`
//! range (see [`money_amount`](super::money_amount)). Output is exact and
//! uncapped. Input parses the decimal string back to `i64` and the currency
//! through `Currency::new`; a string that is non-numeric or overflows `i64` is
//! **refused** with [`GqlValueError::MoneyOutOfRange`], and an unknown ISO code is
//! refused (not coerced) with the currency's own code.

use async_graphql::{InputObject, SimpleObject};
use br_core_values::{Currency, Money};

use crate::values::error::GqlValueError;
use crate::values::money_amount::MoneyAmount;

/// GraphQL **output** projection of a [`Money`]: the full `i64` minor-unit amount
/// (as the [`MoneyAmount`] decimal-string scalar) + the ISO currency code. Built
/// infallibly from a domain [`Money`] — the whole `i64` range round-trips with no
/// ceiling and no truncation.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct GqlMoney {
    /// The amount in the currency's minor unit (centimes, cents, yen), carried as
    /// a decimal string for `i64`-precision safety — see [`MoneyAmount`].
    pub amount: MoneyAmount,
    /// The ISO 4217 currency code (e.g. `EUR`), already validated by the source
    /// [`Money`].
    pub currency: String,
}

impl From<&Money> for GqlMoney {
    /// Project a domain [`Money`] for the wire. Infallible: the full `i64` amount
    /// is carried exactly by the [`MoneyAmount`] string scalar — **never**
    /// truncated, no range bound below `i64`.
    fn from(money: &Money) -> Self {
        Self {
            amount: MoneyAmount(money.amount),
            currency: money.currency.as_str().to_owned(),
        }
    }
}

/// GraphQL **input** for a money value: the minor-unit amount as a decimal
/// **string** (the [`MoneyAmount`] scalar) + a currency code string the client
/// supplies. Converted to a domain [`Money`] via [`TryFrom`], which parses the
/// amount into `i64` and validates the currency.
#[derive(InputObject, Debug, Clone, PartialEq, Eq)]
pub struct GqlMoneyInput {
    /// The amount in the currency's minor unit, as a decimal string. Parsed into
    /// `i64` on conversion; a non-numeric or overflowing string is **refused**.
    pub amount: MoneyAmount,
    /// The ISO 4217 currency code; validated on conversion (an unknown code is
    /// rejected, never coerced).
    pub currency: String,
}

impl TryFrom<GqlMoneyInput> for Money {
    type Error = GqlValueError;

    /// Build a domain [`Money`] from client input. The [`MoneyAmount`] scalar has
    /// already parsed the decimal string into a full `i64` at the GraphQL layer —
    /// a non-numeric or out-of-`i64` string never reaches here (it is rejected as
    /// an input-value error carrying the `MONEY_OUT_OF_RANGE` key). The currency
    /// is validated by `Currency::new`; an unknown/malformed code is **refused**
    /// (wrapped as [`GqlValueError::ValueRejected`], surfacing the value object's
    /// own code — never coerced to a default).
    fn try_from(input: GqlMoneyInput) -> Result<Self, Self::Error> {
        let currency = Currency::new(&input.currency)
            .map_err(|source| GqlValueError::ValueRejected { source })?;
        Ok(Money::new(input.amount.into(), currency))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur() -> Currency {
        Currency::new("EUR").unwrap()
    }

    // ── Output: full i64 round-trips, no truncation ───────────────────────────

    // Given an in-range Money, When projected, Then amount and currency survive.
    #[test]
    fn projects_amount_and_currency() {
        let gql = GqlMoney::from(&Money::new(4250, eur()));
        assert_eq!(gql.amount, MoneyAmount(4250));
        assert_eq!(gql.currency, "EUR");
    }

    #[test]
    fn projects_negative_amount() {
        let gql = GqlMoney::from(&Money::new(-1500, eur()));
        assert_eq!(gql.amount, MoneyAmount(-1500));
    }

    // Given an amount far above i32::MAX, When projected, Then the FULL i64 is
    // carried — no truncation, no ceiling (the inverse of the Hanshow seed, and
    // proof i32 plays no part in the path).
    #[test]
    fn projects_large_i64_without_truncation() {
        let big = i64::from(i32::MAX) * 1_000_000; // ≈ 2.1e15, far past i32 range
        let gql = GqlMoney::from(&Money::new(big, eur()));
        assert_eq!(gql.amount, MoneyAmount(big));
        // And it survives a full domain → wire → domain round-trip unchanged.
        let back = Money::try_from(GqlMoneyInput {
            amount: gql.amount,
            currency: gql.currency,
        })
        .unwrap();
        assert_eq!(back.amount, big);
    }

    // ── Input direction ───────────────────────────────────────────────────────

    // Given valid input, When converted, Then a domain Money is built.
    #[test]
    fn input_builds_domain_money() {
        let money = Money::try_from(GqlMoneyInput {
            amount: MoneyAmount(999),
            currency: "USD".into(),
        })
        .unwrap();
        assert_eq!(money.amount, 999);
        assert_eq!(money.currency.as_str(), "USD");
    }

    // Given an unknown currency in the input, Then conversion is refused — the
    // currency is validated (its own `unknown_currency` code passes through),
    // never coerced to a default.
    #[test]
    fn input_rejects_unknown_currency() {
        let err = Money::try_from(GqlMoneyInput {
            amount: MoneyAmount(100),
            currency: "RMB".into(),
        })
        .unwrap_err();
        assert_eq!(err.reason_code(), "unknown_currency");
    }
}
