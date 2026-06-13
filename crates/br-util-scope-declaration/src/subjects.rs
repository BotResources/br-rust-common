use br_scope_declaration_contract::{
    accepted_subject, command_subject, command_type, rejected_subject,
};

pub(crate) struct DeclarationSubjects {
    pub declare: String,
    pub accepted: String,
    pub rejected: String,
}

impl DeclarationSubjects {
    pub fn build() -> Self {
        Self {
            declare: command_subject()
                .expect("contract subject segments are valid by construction"),
            accepted: accepted_subject()
                .expect("contract subject segments are valid by construction"),
            rejected: rejected_subject()
                .expect("contract subject segments are valid by construction"),
        }
    }

    pub fn confirmation_filters(&self) -> Vec<String> {
        vec![self.accepted.clone(), self.rejected.clone()]
    }

    pub fn command_type() -> String {
        command_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_scope_declaration_contract::VERSION;

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
