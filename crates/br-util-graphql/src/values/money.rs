use async_graphql::{InputObject, SimpleObject};
use br_core_values::{Currency, Money};

use crate::values::error::GqlValueError;
use crate::values::money_amount::MoneyAmount;

#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct GqlMoney {
    pub amount: MoneyAmount,
    pub currency: String,
}

impl From<&Money> for GqlMoney {
    fn from(money: &Money) -> Self {
        Self {
            amount: MoneyAmount(money.amount),
            currency: money.currency.as_str().to_owned(),
        }
    }
}

#[derive(InputObject, Debug, Clone, PartialEq, Eq)]
pub struct GqlMoneyInput {
    pub amount: MoneyAmount,
    pub currency: String,
}

impl TryFrom<GqlMoneyInput> for Money {
    type Error = GqlValueError;

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

    #[test]
    fn projects_large_i64_without_truncation() {
        let big = i64::from(i32::MAX) * 1_000_000;
        let gql = GqlMoney::from(&Money::new(big, eur()));
        assert_eq!(gql.amount, MoneyAmount(big));
        let back = Money::try_from(GqlMoneyInput {
            amount: gql.amount,
            currency: gql.currency,
        })
        .unwrap();
        assert_eq!(back.amount, big);
    }

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
