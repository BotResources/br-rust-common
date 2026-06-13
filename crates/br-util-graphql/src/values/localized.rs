use std::marker::PhantomData;

use async_graphql::InputObject;
use br_core_values::{Localized, LocalizedEntry, ValueError};

use crate::values::error::GqlValueError;
use crate::values::locale::GqlLocale;

#[derive(InputObject, Debug, Clone)]
pub struct GqlLocalizedEntryInput {
    pub locale: String,
    pub content: String,
}

#[derive(InputObject, Debug, Clone)]
pub struct GqlLocalizedInput {
    pub primary: String,
    pub entries: Vec<GqlLocalizedEntryInput>,
}

impl GqlLocalizedInput {
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

    #[test]
    fn refuses_unknown_primary_locale() {
        let input = GqlLocalizedInput {
            primary: "xx".into(),
            entries: vec![entry("xx", "x")],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err, GqlValueError::LocaleUnknown { value: "xx".into() });
    }

    #[test]
    fn refuses_unknown_entry_locale() {
        let input = GqlLocalizedInput {
            primary: "en".into(),
            entries: vec![entry("en", "Hello"), entry("xx", "?")],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err, GqlValueError::LocaleUnknown { value: "xx".into() });
    }

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

    #[test]
    fn refuses_empty_entries_with_its_own_code() {
        let input = GqlLocalizedInput {
            primary: "en".into(),
            entries: vec![],
        };
        let err = input.into_localized::<Markdown, Locale>().unwrap_err();
        assert_eq!(err.reason_code(), "localized_empty");
    }

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
