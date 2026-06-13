use br_core_integration::{MessageKind, integration_subject};

pub(crate) const VERSION: u8 = 1;

const BC: &str = "identity";
const AGGREGATE: &str = "service_scope";
const COMMAND_NAME: &str = "declare";

pub(crate) struct DeclarationSubjects {
    pub declare: String,
    pub accepted: String,
    pub rejected: String,
}

impl DeclarationSubjects {
    pub fn build() -> Self {
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

    pub fn confirmation_filters(&self) -> Vec<String> {
        vec![self.accepted.clone(), self.rejected.clone()]
    }

    pub fn command_type() -> String {
        format!("{AGGREGATE}.{COMMAND_NAME}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let s = DeclarationSubjects::build();
        assert_eq!(s.declare, format!("identity.cmd.{command_type}.v{VERSION}"));
    }
}
