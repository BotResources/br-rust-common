//! [`GqlLocale`] — the seam by which the lib parses a client locale string into
//! the product's own closed `Locale` enum, **fallibly**.
//!
//! The lib owns no locale list (neither does `br-core-values`); each product
//! supplies its `Locale` enum. To convert a wire locale string into that enum
//! the product implements this one-method trait — typically by delegating to its
//! enum's serde representation. The contract that matters here: an **unknown**
//! locale returns `None`, so the wrappers can refuse it
//! ([`GqlValueError::LocaleUnknown`]) rather than coerce it to a default (the
//! Hanshow anti-model).
//!
//! [`GqlValueError::LocaleUnknown`]: crate::values::GqlValueError::LocaleUnknown

use crate::values::error::GqlValueError;

/// Parse a wire locale string into the product's closed `Locale` enum.
///
/// Implement it on the product's `Locale` (the lib provides no list). Return
/// `None` for any string that is not a known locale — **never** a default — so
/// the conversion can be refused with a typed code.
pub trait GqlLocale: Sized {
    /// Parse the wire string (e.g. `"en"`, `"fr"`, `"ja"`) into a locale, or
    /// `None` if it matches no variant.
    fn from_wire(s: &str) -> Option<Self>;

    /// Parse, or refuse with [`GqlValueError::LocaleUnknown`] carrying the input.
    /// Provided — do not override.
    fn parse_wire(s: &str) -> Result<Self, GqlValueError> {
        Self::from_wire(s).ok_or_else(|| GqlValueError::LocaleUnknown {
            value: s.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A product's three-locale enum, the way a consumer would write it.
    #[derive(Debug, PartialEq, Eq)]
    enum Locale {
        En,
        Fr,
        Ja,
    }

    impl GqlLocale for Locale {
        fn from_wire(s: &str) -> Option<Self> {
            match s {
                "en" => Some(Locale::En),
                "fr" => Some(Locale::Fr),
                "ja" => Some(Locale::Ja),
                _ => None,
            }
        }
    }

    // Given a known locale, Then it parses.
    #[test]
    fn parses_a_known_locale() {
        assert_eq!(Locale::parse_wire("fr").unwrap(), Locale::Fr);
    }

    // Given an unknown locale, Then it is REFUSED with LOCALE_UNKNOWN carrying
    // the input — never coerced to a default (the inverse of the Hanshow seed).
    #[test]
    fn refuses_an_unknown_locale_with_the_input() {
        let err = Locale::parse_wire("xx").unwrap_err();
        assert_eq!(err, GqlValueError::LocaleUnknown { value: "xx".into() });
        assert_eq!(err.reason_code(), "LOCALE_UNKNOWN");
    }
}
