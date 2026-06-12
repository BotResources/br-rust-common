//! ISO-backed value objects: [`Currency`] (ISO 4217), [`CountryCode`]
//! (ISO 3166-1 alpha-2), and [`Money`] (minor-unit `i64` + currency).
//!
//! Each is a constructor-validated newtype that makes an illegal state
//! unrepresentable; the underlying authoritative code lists live in
//! [`currency_codes`] / [`country_codes`] and are exposed for callers that need
//! to enumerate the supported set.

pub mod country_codes;
pub mod currency_codes;

mod country;
mod currency;
mod money;

pub use country::CountryCode;
pub use currency::Currency;
pub use money::Money;
