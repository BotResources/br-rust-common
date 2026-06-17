use crate::error::ValueError;

pub trait LocaleCodec: Sized {
    fn from_wire(s: &str) -> Option<Self>;

    fn as_wire(&self) -> &str;

    fn parse_wire(s: &str) -> Result<Self, ValueError> {
        Self::from_wire(s).ok_or_else(|| ValueError::LocaleUnknown {
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

    impl LocaleCodec for Locale {
        fn from_wire(s: &str) -> Option<Self> {
            match s {
                "en" => Some(Locale::En),
                "fr" => Some(Locale::Fr),
                "ja" => Some(Locale::Ja),
                _ => None,
            }
        }

        fn as_wire(&self) -> &str {
            match self {
                Locale::En => "en",
                Locale::Fr => "fr",
                Locale::Ja => "ja",
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
        assert_eq!(err, ValueError::LocaleUnknown { value: "xx".into() });
        assert_eq!(err.to_string(), "locale_unknown");
    }

    #[test]
    fn as_wire_round_trips_through_from_wire() {
        for code in ["en", "fr", "ja"] {
            assert_eq!(Locale::from_wire(code).unwrap().as_wire(), code);
        }
    }
}
