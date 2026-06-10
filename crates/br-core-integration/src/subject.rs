//! Builder for the integration subject convention — a mechanism, so the rule
//! is enforced once here instead of restated as prose at every call site.
//!
//! Convention:
//! - Events:   `{bc}.evt.{aggregate}.{name}.v{N}`  — e.g. `identity.evt.user.created.v1`
//! - Commands: `{bc}.cmd.{aggregate}.{name}.v{N}`  — e.g. `notifier.cmd.notification.send.v1`

/// Whether a subject names a command (`cmd`) or an event (`evt`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    /// A request to a receiving context (`cmd`). Contract owned by the receiver.
    Cmd,
    /// A fact emitted by a producing context (`evt`). Contract owned by the producer.
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

/// Why a subject segment was rejected. The builder returns a `Result` rather
/// than panicking: this crate is the platform foundation, and a malformed
/// subject is a caller error to surface, not a process abort.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum SubjectError {
    /// A segment was empty.
    #[error("subject segment '{segment}' is empty")]
    Empty { segment: &'static str },
    /// A segment contained a character outside the allowed set `[a-z0-9_-]`.
    /// The whitelist keeps every NATS-significant character out: `.` (the
    /// segment separator), the `*`/`>` wildcards, spaces, and control bytes
    /// can neither break the subject structure nor collide with a
    /// subscriber's wildcard semantics. Underscore is allowed: multi-word
    /// aggregates are snake_case (e.g. `service_scope`).
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

/// Build a subject following the integration convention:
/// `{bc}.{cmd|evt}.{aggregate}.{name}.v{version}`.
///
/// Each of `bc`, `aggregate`, `name` must be non-empty and drawn from the
/// charset `[a-z0-9_-]` — anything else (uppercase, `.`, the NATS wildcards
/// `*`/`>`, whitespace) returns a [`SubjectError`] rather than emitting a
/// malformed or wildcard-colliding subject. Multi-word segments are
/// snake_case (e.g. `service_scope`).
///
/// ```
/// use br_core_integration::{integration_subject, MessageKind};
///
/// let s = integration_subject("identity", MessageKind::Evt, "user", "created", 1).unwrap();
/// assert_eq!(s, "identity.evt.user.created.v1");
/// ```
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

    // NATS-significant characters must never reach a subject: a `*` or `>`
    // would collide with subscriber wildcard semantics, a space is invalid.
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

    // Multi-word aggregates are snake_case in the shared contract
    // (`identity.cmd.service_scope.declare.v1`) — the builder must produce them.
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
