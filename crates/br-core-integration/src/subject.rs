#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Cmd,
    Evt,
}

impl MessageKind {
    const fn token(self) -> &'static str {
        match self {
            Self::Cmd => "cmd",
            Self::Evt => "evt",
        }
    }
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum SubjectError {
    #[error("subject segment '{segment}' is empty")]
    Empty { segment: &'static str },
    #[error("subject segment '{segment}' contains a character outside [a-z0-9_-]: {value:?}")]
    InvalidChar {
        segment: &'static str,
        value: String,
    },
}

fn validate(segment: &'static str, value: &str) -> Result<(), SubjectError> {
    if value.is_empty() {
        return Err(SubjectError::Empty { segment });
    }
    if value
        .chars()
        .any(|c| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_'))
    {
        return Err(SubjectError::InvalidChar {
            segment,
            value: value.to_string(),
        });
    }
    Ok(())
}

pub fn integration_subject(
    bc: &str,
    kind: MessageKind,
    aggregate: &str,
    name: &str,
    version: u8,
) -> Result<String, SubjectError> {
    validate("bc", bc)?;
    validate("aggregate", aggregate)?;
    validate("name", name)?;
    Ok(format!(
        "{bc}.{}.{aggregate}.{name}.v{version}",
        kind.token()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_event_subject() {
        let s = integration_subject("identity", MessageKind::Evt, "user", "created", 1).unwrap();
        assert_eq!(s, "identity.evt.user.created.v1");
    }

    #[test]
    fn builds_command_subject() {
        let s =
            integration_subject("notifier", MessageKind::Cmd, "notification", "send", 2).unwrap();
        assert_eq!(s, "notifier.cmd.notification.send.v2");
    }

    #[test]
    fn rejects_empty_segment() {
        let err = integration_subject("", MessageKind::Evt, "user", "created", 1).unwrap_err();
        assert_eq!(err, SubjectError::Empty { segment: "bc" });
    }

    #[test]
    fn rejects_dot_in_segment() {
        let err = integration_subject("identity", MessageKind::Evt, "user.profile", "created", 1)
            .unwrap_err();
        assert!(matches!(
            err,
            SubjectError::InvalidChar {
                segment: "aggregate",
                ..
            }
        ));
    }

    #[test]
    fn rejects_uppercase_segment() {
        let err =
            integration_subject("identity", MessageKind::Evt, "user", "Created", 1).unwrap_err();
        assert!(matches!(
            err,
            SubjectError::InvalidChar {
                segment: "name",
                ..
            }
        ));
    }

    #[test]
    fn rejects_nats_wildcards_and_whitespace() {
        for bad in ["user*created", "user>", "user created"] {
            let err =
                integration_subject("identity", MessageKind::Evt, "user", bad, 1).expect_err(bad);
            assert!(matches!(
                err,
                SubjectError::InvalidChar {
                    segment: "name",
                    ..
                }
            ));
        }
    }

    #[test]
    fn accepts_digits_and_hyphens() {
        let s = integration_subject("identity", MessageKind::Evt, "user-profile", "created2", 1)
            .unwrap();
        assert_eq!(s, "identity.evt.user-profile.created2.v1");
    }

    #[test]
    fn accepts_snake_case_segments() {
        let s = integration_subject("identity", MessageKind::Cmd, "service_scope", "declare", 1)
            .unwrap();
        assert_eq!(s, "identity.cmd.service_scope.declare.v1");
    }

    #[test]
    fn version_renders_as_v_n() {
        let s = integration_subject("bc", MessageKind::Evt, "agg", "name", 42).unwrap();
        assert!(s.ends_with(".v42"));
    }
}
