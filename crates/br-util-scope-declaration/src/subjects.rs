//! The three handshake subjects — fixed by the **published contract** in
//! `br-core-scope`:
//!
//! - command:  `identity.cmd.service_scope.declare.v1`
//! - accepted: `identity.evt.service_scope.accepted.v1`
//! - rejected: `identity.evt.service_scope.rejected.v1`
//!
//! Built with [`integration_subject`] — the single source of the subject
//! convention — and pinned to the canonical contract strings by a unit test,
//! so neither this crate nor the builder can drift from what Identity speaks.

use br_core_integration::{MessageKind, integration_subject};

/// The wire schema version of the handshake messages (the `vN` segment).
pub(crate) const VERSION: u8 = 1;

const BC: &str = "identity";
const AGGREGATE: &str = "service_scope";
const COMMAND_NAME: &str = "declare";

/// The concrete subjects of the scope-declaration handshake.
pub(crate) struct DeclarationSubjects {
    /// `identity.cmd.service_scope.declare.v1` — the durable declare command.
    pub declare: String,
    /// `identity.evt.service_scope.accepted.v1` — the accepted confirmation.
    pub accepted: String,
    /// `identity.evt.service_scope.rejected.v1` — the rejected confirmation.
    pub rejected: String,
}

impl DeclarationSubjects {
    /// The handshake subjects, fixed by the published contract.
    pub fn build() -> Self {
        // Inputs are crate-internal literals, valid for the builder's charset
        // by construction; the contract-pinning test below proves it. An error
        // here is unreachable, not a runtime condition to handle.
        let subject = |kind, name| {
            integration_subject(BC, kind, AGGREGATE, name, VERSION)
                .expect("contract subject segments are valid by construction")
        };
        Self {
            declare: subject(MessageKind::Cmd, COMMAND_NAME),
            accepted: subject(MessageKind::Evt, "accepted"),
            rejected: subject(MessageKind::Evt, "rejected"),
        }
    }

    /// The two confirmation subjects to filter the awaiter on (accepted +
    /// rejected). One consumer awaits either.
    pub fn confirmation_filters(&self) -> Vec<String> {
        vec![self.accepted.clone(), self.rejected.clone()]
    }

    /// The `command_type` stamped on the command envelope —
    /// `{aggregate}.{name}`, derived from the same constants as the subject,
    /// so the two cannot drift apart.
    pub fn command_type() -> String {
        format!("{AGGREGATE}.{COMMAND_NAME}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The lock that matters: the builder output IS the published contract.
    // A drift in either the builder or these segments fails here.
    #[test]
    fn subjects_match_the_published_contract() {
        let s = DeclarationSubjects::build();
        assert_eq!(s.declare, "identity.cmd.service_scope.declare.v1");
        assert_eq!(s.accepted, "identity.evt.service_scope.accepted.v1");
        assert_eq!(s.rejected, "identity.evt.service_scope.rejected.v1");
    }

    #[test]
    fn confirmation_filters_are_the_two_event_subjects() {
        let s = DeclarationSubjects::build();
        assert_eq!(
            s.confirmation_filters(),
            vec![
                "identity.evt.service_scope.accepted.v1".to_string(),
                "identity.evt.service_scope.rejected.v1".to_string(),
            ]
        );
    }

    #[test]
    fn command_type_matches_the_subject_tail() {
        let command_type = DeclarationSubjects::command_type();
        assert_eq!(command_type, "service_scope.declare");
        // Mechanical link to the subject: `{bc}.cmd.{command_type}.v{N}`.
        let s = DeclarationSubjects::build();
        assert_eq!(s.declare, format!("identity.cmd.{command_type}.v{VERSION}"));
    }
}
