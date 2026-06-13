use br_core_integration::{MessageKind, SubjectError, integration_subject};

pub const BC: &str = "identity";
pub const AGGREGATE: &str = "service_scope";
pub const VERSION: u8 = 1;
pub const COMMAND_NAME: &str = "declare";
pub const ACCEPTED: &str = "accepted";
pub const REJECTED: &str = "rejected";

pub fn command_subject() -> Result<String, SubjectError> {
    integration_subject(BC, MessageKind::Cmd, AGGREGATE, COMMAND_NAME, VERSION)
}

pub fn event_subject(name: &str) -> Result<String, SubjectError> {
    integration_subject(BC, MessageKind::Evt, AGGREGATE, name, VERSION)
}

pub fn accepted_subject() -> Result<String, SubjectError> {
    event_subject(ACCEPTED)
}

pub fn rejected_subject() -> Result<String, SubjectError> {
    event_subject(REJECTED)
}

pub fn command_type() -> String {
    format!("{AGGREGATE}.{COMMAND_NAME}")
}

pub fn event_type(name: &str) -> String {
    format!("{AGGREGATE}.{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_match_the_published_wire_contract() {
        assert_eq!(
            command_subject().unwrap(),
            "identity.cmd.service_scope.declare.v1"
        );
        assert_eq!(
            accepted_subject().unwrap(),
            "identity.evt.service_scope.accepted.v1"
        );
        assert_eq!(
            rejected_subject().unwrap(),
            "identity.evt.service_scope.rejected.v1"
        );
    }

    #[test]
    fn command_type_is_the_subject_tail() {
        assert_eq!(command_type(), "service_scope.declare");
        assert_eq!(
            command_subject().unwrap(),
            format!("identity.cmd.{}.v{VERSION}", command_type())
        );
    }

    #[test]
    fn event_type_carries_the_aggregate_prefix() {
        assert_eq!(event_type(ACCEPTED), "service_scope.accepted");
        assert_eq!(event_type(REJECTED), "service_scope.rejected");
    }
}
