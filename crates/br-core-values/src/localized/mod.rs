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
//! ## Locale wire form is the product's responsibility
//!
//! The family serializes the locale via `L`'s **own** serde representation; the
//! lib imposes none. A product must give its `Locale` enum a single, stable wire
//! form — the recommendation is lowercase (`"en"`/`"fr"`) matching the BCP-47
//! convention, with `#[serde(alias = …)]` read-compat for any earlier form
//! already persisted in events. Owning that here would mean owning the locale
//! list, which the family deliberately does not.

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
