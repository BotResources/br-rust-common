use crate::values::error::GqlValueError;

pub trait GqlLocale: Sized {
    fn from_wire(s: &str) -> Option<Self>;

    fn parse_wire(s: &str) -> Result<Self, GqlValueError> {
        Self::from_wire(s).ok_or_else(|| GqlValueError::LocaleUnknown {
            value: s.to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn parses_a_known_locale() {
        assert_eq!(Locale::parse_wire("fr").unwrap(), Locale::Fr);
    }

    #[test]
    fn refuses_an_unknown_locale_with_the_input() {
        let err = Locale::parse_wire("xx").unwrap_err();
        assert_eq!(err, GqlValueError::LocaleUnknown { value: "xx".into() });
        assert_eq!(err.reason_code(), "LOCALE_UNKNOWN");
    }
}
