use crate::coords::segment::{CoordError, validate_segment};

macro_rules! coord_newtype {
    ($name:ident, $role:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, CoordError> {
                let value = value.into();
                validate_segment($role, &value)?;
                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl std::convert::TryFrom<&str> for $name {
            type Error = CoordError;
            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::new(value)
            }
        }
    };
}

coord_newtype!(Bc, "bc");
coord_newtype!(Aggregate, "aggregate");
coord_newtype!(Verb, "verb");
coord_newtype!(PastFact, "fact");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_segments_construct() {
        assert_eq!(Bc::new("identity").unwrap().as_str(), "identity");
        assert_eq!(Aggregate::new("user").unwrap().as_str(), "user");
        assert_eq!(Verb::new("declare").unwrap().as_str(), "declare");
        assert_eq!(PastFact::new("created").unwrap().as_str(), "created");
    }

    #[test]
    fn invalid_segments_are_rejected_with_the_right_role() {
        assert_eq!(Bc::new(""), Err(CoordError::Empty { role: "bc" }));
        assert!(matches!(
            Aggregate::new("user.profile"),
            Err(CoordError::InvalidChar {
                role: "aggregate",
                ..
            })
        ));
        assert!(matches!(
            Verb::new("send*"),
            Err(CoordError::InvalidChar { role: "verb", .. })
        ));
        assert!(matches!(
            PastFact::new("done>"),
            Err(CoordError::InvalidChar { role: "fact", .. })
        ));
    }

    #[test]
    fn try_from_str_delegates_to_new() {
        let bc: Bc = "notifier".try_into().unwrap();
        assert_eq!(bc.as_str(), "notifier");
        let bad: Result<Bc, _> = "no.tifier".try_into();
        assert!(bad.is_err());
    }
}
