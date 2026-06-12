//! [`GqlLocalizedInput`] — a **fallible** GraphQL input that builds a
//! [`Localized<F, L>`] value, refusing missing primary content.
//!
//! The anti-model (Hanshow) accepted a localized value with no entry for its
//! declared primary locale and coerced unknown locales to the default. This
//! wrapper does the **opposite**: every locale string is parsed through the
//! product's [`GqlLocale`] seam (an unknown one is refused with
//! [`GqlValueError::LocaleUnknown`]), and the value is assembled through the
//! value object's validating constructor `Localized::from_parts` — a missing
//! primary entry is refused with [`GqlValueError::PrimaryContentMissing`], never
//! silently accepted.
//!
//! Generic over the format marker `F` (so a service builds a `LocalizedString` /
//! `LocalizedMd` / `LocalizedHtml`) and the product's locale `L: GqlLocale`.

use std::marker::PhantomData;

use async_graphql::InputObject;
use br_core_values::{Localized, LocalizedEntry, ValueError};

use crate::values::error::GqlValueError;
use crate::values::locale::GqlLocale;

/// One wire entry: a locale string and its content. The locale is parsed (and
/// possibly refused) on conversion.
#[derive(InputObject, Debug, Clone)]
pub struct GqlLocalizedEntryInput {
    /// The wire locale string (e.g. `"en"`).
    pub locale: String,
    /// The content for that locale, in the format the target marker declares.
    pub content: String,
}

/// GraphQL input for a localized value: a primary locale string plus per-locale
/// entries. Converted to a domain [`Localized<F, L>`] via [`GqlLocalizedInput::into_localized`].
#[derive(InputObject, Debug, Clone)]
pub struct GqlLocalizedInput {
    /// The wire string of the primary locale.
    pub primary: String,
    /// The per-locale entries (must include one for `primary`).
    pub entries: Vec<GqlLocalizedEntryInput>,
}

impl GqlLocalizedInput {
    /// Build a domain [`Localized<F, L>`] from this input.
    ///
    /// Picks the format marker `F` (e.g. `Markdown`) and the product's locale
    /// `L` at the call site. Every locale string is parsed through
    /// [`GqlLocale`]; the value is assembled through the validating
    /// `Localized::from_parts`.
    ///
    /// # Errors
    /// - [`GqlValueError::LocaleUnknown`] if any locale string is not a known
    ///   locale (never coerced to a default).
    /// - [`GqlValueError::PrimaryContentMissing`] if no entry matches `primary`.
    /// - [`GqlValueError::ValueRejected`] for the other value-object rejections
    ///   (empty entries, a duplicate locale), surfacing their own stable codes.
    pub fn into_localized<F, L>(self) -> Result<Localized<F, L>, GqlValueError>
    where
        L: GqlLocale + PartialEq,
    {
        let primary = L::parse_wire(&self.primary)?;
        let mut entries = Vec::with_capacity(self.entries.len());
        for entry in self.entries {
            entries.push(LocalizedEntry {
                locale: L::parse_wire(&entry.locale)?,
                content: entry.content,
            });
        }
        let _ = PhantomData::<F>;
        Localized::<F, L>::from_parts(primary, entries).map_err(map_localized_error)
    }
}

/// Map a `Localized::from_parts` rejection to a typed conversion error: the
/// missing-primary case to the named [`GqlValueError::PrimaryContentMissing`],
/// the rest to [`GqlValueError::ValueRejected`] (their own codes pass through).
fn map_localized_error(error: ValueError) -> GqlValueError {
    match error {
        ValueError::LocalizedPrimaryMissing => GqlValueError::PrimaryContentMissing,
        other => GqlValueError::ValueRejected { source: other },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_values::Markdown;

    #[derive(Debug, PartialEq, Eq)]
    enum Locale {
        En,
        Fr,
    }

    impl GqlLocale for Locale {
        fn from_wire(s: &str) -> Option<Self> {
            match s {
                "en" => Some(Locale::En),
                "fr" => Some(Locale::Fr),
                _ => None,
            }
        }
    }

    fn entry(locale: &str, content: &str) -> GqlLocalizedEntryInput {
        GqlLocalizedEntryInput {
            locale: locale.into(),
            content: content.into(),
        }
    }

    // ── Success ───────────────────────────────────────────────────────────────

    // Given a primary with a matching entry, Then a valid Localized is built.
    #[test]
    fn builds_a_valid_localized_value() {
        let input = GqlLocalizedInput {
            primary: "en".into(),
            entries: vec![entry("en", "Hello"), entry("fr", "Bonjour")],
        };
        let localized: Localized<Markdown, Locale> = input.into_localized().unwrap();
        assert_eq!(localized.primary(), "Hello");
        assert_eq!(localized.get(&Locale::Fr), Some("Bonjour"));
    }

    // ── Failure: LOCALE_UNKNOWN (never coerce) ────────────────────────────────

    // Given an unknown primary locale, Then it is REFUSED — never defaulted.
    #[test]
    fn refuses_unknown_primary_locale() {
        let input = GqlLocalizedInput {
            primary: "xx".into(),
            entries: vec![entry("xx", "x")],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err, GqlValueError::LocaleUnknown { value: "xx".into() });
    }

    // Given an unknown locale on a non-primary entry, Then it too is refused.
    #[test]
    fn refuses_unknown_entry_locale() {
        let input = GqlLocalizedInput {
            primary: "en".into(),
            entries: vec![entry("en", "Hello"), entry("xx", "?")],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err, GqlValueError::LocaleUnknown { value: "xx".into() });
    }

    // ── Failure: PRIMARY_CONTENT_MISSING (never accept) ───────────────────────

    // Given a primary with no matching entry, Then it is REFUSED with
    // PRIMARY_CONTENT_MISSING — the inverse of the Hanshow seed.
    #[test]
    fn refuses_missing_primary_content() {
        let input = GqlLocalizedInput {
            primary: "en".into(),
            entries: vec![entry("fr", "Bonjour")],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err, GqlValueError::PrimaryContentMissing);
        assert_eq!(err.reason_code(), "PRIMARY_CONTENT_MISSING");
    }

    // Given no entries at all, Then the empty rejection surfaces its own code.
    #[test]
    fn refuses_empty_entries_with_its_own_code() {
        let input = GqlLocalizedInput {
            primary: "en".into(),
            entries: vec![],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err.reason_code(), "localized_empty");
    }

    // Given a duplicate locale, Then the duplicate rejection surfaces its code.
    #[test]
    fn refuses_duplicate_locale_with_its_own_code() {
        let input = GqlLocalizedInput {
            primary: "en".into(),
            entries: vec![entry("en", "Hello"), entry("en", "Hi")],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err.reason_code(), "localized_duplicate_locale");
    }
}
