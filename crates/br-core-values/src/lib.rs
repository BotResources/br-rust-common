//! Universal, constructor-validated **value objects** shared across BotResources
//! services. Tier `core` — serde only, **no I/O, no `async`, and no dependency
//! on any `br-util-*` crate**. Two families:
//!
//! ## The `Localized<F, L>` rich-text family (generic over locale)
//!
//! Text available in one or more locales, with one primary. Generic over a
//! type-level **format** marker `F` (plain / markdown / html) and the **locale**
//! type `L` — the lib owns no locale list, each product instantiates with its
//! own closed `Locale` enum. Use the aliases:
//!
//! - [`LocalizedString<L>`] — plain text.
//! - [`LocalizedMd<L>`] — markdown.
//! - [`LocalizedHtml<L>`] — raw html (sanitized at the render edge, never here).
//! - [`LocalizedContent<L>`] — a runtime tagged union over md/html.
//!
//! Three invariants (≥1 entry, primary present, no duplicate locale) are
//! enforced at construction **and** re-validated on every deserialization — so
//! serde (the main constructor path in an event-logged system) cannot smuggle in
//! an illegal value. See the [`localized`] module for the format/locale model.
//!
//! ## ISO-backed value objects
//!
//! - [`Currency`] — ISO 4217 alphabetic code (169 active codes).
//! - [`CountryCode`] — ISO 3166-1 alpha-2 code (249 codes).
//! - [`Money`] — minor-unit `i64` amount + [`Currency`] (no arithmetic here).
//!
//! Each is a self-validating newtype; an illegal value can be neither
//! constructed nor deserialized.
//!
//! ## Errors
//!
//! Every constructor returns this crate's own [`ValueError`] — stable codes,
//! structured params, **never UI prose** (codes-not-language: the human text and
//! its i18n live at the edge). It is `#[non_exhaustive]`; match with a wildcard.

mod error;
mod iso;
mod localized;

pub use error::ValueError;
pub use iso::country_codes::COUNTRY_CODES;
pub use iso::currency_codes::CURRENCY_CODES;
pub use iso::{CountryCode, Currency, Money};
pub use localized::{
    Html, Localized, LocalizedContent, LocalizedEntry, LocalizedHtml, LocalizedMd, LocalizedString,
    Markdown, PlainText, TextFormat,
};
