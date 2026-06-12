//! Fallible GraphQL wrappers over `br-core-values`.
//!
//! Every conversion here **fails with a typed code, never silently coerces** —
//! the deliberate inverse of the Hanshow seed (which coerced unknown locales to
//! the default, accepted missing primary content, and truncated `Money`
//! i64→i32). The three named conversion codes are `LOCALE_UNKNOWN`,
//! `MONEY_OUT_OF_RANGE`, `PRIMARY_CONTENT_MISSING`; a value-object rejection that
//! is none of these passes its own stable code through ([`GqlValueError::ValueRejected`]).
//!
//! - [`GqlLocale`] — the product-supplied seam to parse a wire locale string
//!   into its closed `Locale` enum (refusing unknowns).
//! - [`GqlLocalizedInput`] — builds a `Localized<F, L>`, refusing an unknown
//!   locale and a missing primary entry.
//! - [`GqlMoney`] / [`GqlMoneyInput`] — project / accept a `Money`, carrying the
//!   full `i64` amount through the [`MoneyAmount`] decimal-string scalar (no
//!   truncation, no ceiling), refusing an unparsable amount and an unknown
//!   currency.

mod error;
mod locale;
mod localized;
mod money;
mod money_amount;

pub use error::GqlValueError;
pub use locale::GqlLocale;
pub use localized::{GqlLocalizedEntryInput, GqlLocalizedInput};
pub use money::{GqlMoney, GqlMoneyInput};
pub use money_amount::MoneyAmount;
