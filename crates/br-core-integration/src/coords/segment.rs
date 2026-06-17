#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum CoordError {
    #[error("coordinate segment '{role}' is empty")]
    Empty { role: &'static str },
    #[error("coordinate segment '{role}' contains a character outside [A-Za-z0-9_-]: {value:?}")]
    InvalidChar { role: &'static str, value: String },
}

pub(crate) fn validate_segment(role: &'static str, value: &str) -> Result<(), CoordError> {
    if value.is_empty() {
        return Err(CoordError::Empty { role });
    }
    if value
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    {
        return Err(CoordError::InvalidChar {
            role,
            value: value.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_alnum_hyphen_underscore() {
        assert!(validate_segment("bc", "identity").is_ok());
        assert!(validate_segment("aggregate", "user-profile").is_ok());
        assert!(validate_segment("verb", "service_scope").is_ok());
        assert!(validate_segment("fact", "Created2").is_ok());
    }

    #[test]
    fn rejects_empty() {
        assert_eq!(
            validate_segment("bc", ""),
            Err(CoordError::Empty { role: "bc" })
        );
    }

    #[test]
    fn rejects_dot() {
        assert!(matches!(
            validate_segment("aggregate", "user.profile"),
            Err(CoordError::InvalidChar {
                role: "aggregate",
                ..
            })
        ));
    }

    #[test]
    fn rejects_nats_wildcards() {
        for bad in ["user*", "user>", ">", "*"] {
            assert!(
                matches!(
                    validate_segment("aggregate", bad),
                    Err(CoordError::InvalidChar { .. })
                ),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn rejects_whitespace() {
        assert!(matches!(
            validate_segment("verb", "send now"),
            Err(CoordError::InvalidChar { .. })
        ));
    }
}
