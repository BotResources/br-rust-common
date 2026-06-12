//! The `Localized<F, L>` rich-text family, generic over the locale type.
//!
//! The lib owns **no locale list** — each product supplies its own closed
//! `Locale` enum (`En`/`Fr`/`Ja` here, `En`/`Zh` there) and instantiates the
//! family with it. Pick the format with the type aliases below; the locale `L`
//! is your enum.
//!
//! - [`LocalizedString<L>`] — plain text (titles, names, labels).
//! - [`LocalizedMd<L>`] — markdown (descriptions, summaries, prompt outputs).
//! - [`LocalizedHtml<L>`] — raw html (interactive reports; sanitized at render).
//! - [`LocalizedContent<L>`] — a runtime tagged union over md/html.
//!
//! All three invariants (≥1 entry, primary present, no duplicate locale) are
//! enforced at construction **and** re-validated on every deserialization — see
//! [`Localized`] for the rationale (serde is the main constructor path in an
//! event-logged system).
//!
//! ## Locale wire form: lowercase, by norm
//!
//! The family serializes the locale via `L`'s **own** serde representation; the
//! lib owns no locale *list*, but it does own the casing **norm**: a language
//! locale is the ASCII-**lowercase** ISO 639-1 / BCP 47 language subtag
//! (`"en"`/`"fr"`/`"ja"`) — distinct from the UPPERCASE
//! [`CountryCode`](crate::CountryCode) (ISO 3166-1) and
//! [`Currency`](crate::Currency) (ISO 4217). A product must give its `Locale`
//! enum that single, stable lowercase wire form, with `#[serde(alias = …)]`
//! read-compat for any earlier (capitalized) form already persisted in events.
//! With the `conformance` feature, `assert_lowercase_roundtrip` proves the
//! product's enum obeys the norm from the product's own tests.

mod content;
mod entry;
mod format;
mod value;

pub use content::LocalizedContent;
pub use entry::LocalizedEntry;
pub use format::{Html, Markdown, PlainText, TextFormat};
pub use value::Localized;

/// Plain-text localized value (titles, names, labels).
pub type LocalizedString<L> = Localized<PlainText, L>;
/// Markdown localized value (descriptions, summaries, prompt outputs).
pub type LocalizedMd<L> = Localized<Markdown, L>;
/// Raw-HTML localized value (interactive reports; sanitized at the render edge).
pub type LocalizedHtml<L> = Localized<Html, L>;

#[cfg(test)]
mod tests;
