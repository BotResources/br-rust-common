use br_core_integration::{CommandCoords, EventCoords};
use br_scope_declaration_contract::{
    accepted_event_coords, command_type, declare_command_coords, rejected_event_coords,
};
use br_util_nats_fabric::event_subject;

pub(crate) struct DeclarationCoords {
    pub declare: CommandCoords,
    pub accepted: EventCoords,
    pub rejected: EventCoords,
    pub accepted_subject: String,
}

impl DeclarationCoords {
    pub fn build() -> Self {
        let accepted = accepted_event_coords().expect("contract coordinates are valid");
        let rejected = rejected_event_coords().expect("contract coordinates are valid");
        let accepted_subject = event_subject(&accepted);
        Self {
            declare: declare_command_coords().expect("contract coordinates are valid"),
            accepted,
            rejected,
            accepted_subject,
        }
    }

    pub fn confirmation_coords(&self) -> [&EventCoords; 2] {
        [&self.accepted, &self.rejected]
    }

    pub fn command_type() -> String {
        command_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirmation_subjects_are_the_two_fabric_event_subjects() {
        let c = DeclarationCoords::build();
        assert_eq!(
            c.accepted_subject,
            "integration.evt.identity.service_scope.accepted.v1"
        );
        assert_eq!(
            event_subject(&c.rejected),
            "integration.evt.identity.service_scope.rejected.v1"
        );
    }

    #[test]
    fn declare_command_renders_the_fabric_command_subject() {
        let c = DeclarationCoords::build();
        assert_eq!(
            br_util_nats_fabric::command_subject(&c.declare),
            "integration.cmd.identity.service_scope.declare.v1"
        );
    }

    #[test]
    fn command_type_is_the_aggregate_verb_pair() {
        assert_eq!(DeclarationCoords::command_type(), "service_scope.declare");
    }
}
