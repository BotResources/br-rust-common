#![doc = include_str!("../README.md")]

use br_core_integration::{Aggregate, Bc, CommandCoords, CoordError, EventCoords, PastFact, Verb};

pub const BC: &str = "identity";
pub const AGGREGATE: &str = "service_scope";
pub const VERSION: u8 = 1;
pub const COMMAND_NAME: &str = "declare";
pub const ACCEPTED: &str = "accepted";
pub const REJECTED: &str = "rejected";
pub const UNREPRESENTABLE_SERVICE: &str = "unrepresentable_service";

pub fn declare_command_coords() -> Result<CommandCoords, CoordError> {
    Ok(CommandCoords {
        receiver: Bc::new(BC)?,
        aggregate: Aggregate::new(AGGREGATE)?,
        verb: Verb::new(COMMAND_NAME)?,
        version: VERSION,
    })
}

pub fn accepted_event_coords() -> Result<EventCoords, CoordError> {
    event_coords(ACCEPTED)
}

pub fn rejected_event_coords() -> Result<EventCoords, CoordError> {
    event_coords(REJECTED)
}

fn event_coords(fact: &str) -> Result<EventCoords, CoordError> {
    Ok(EventCoords {
        producer: Bc::new(BC)?,
        aggregate: Aggregate::new(AGGREGATE)?,
        fact: PastFact::new(fact)?,
        version: VERSION,
    })
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
    fn declare_command_coords_carry_the_typed_v1_grammar() {
        let coords = declare_command_coords().unwrap();
        assert_eq!(coords.receiver.as_str(), "identity");
        assert_eq!(coords.aggregate.as_str(), "service_scope");
        assert_eq!(coords.verb.as_str(), "declare");
        assert_eq!(coords.version, 1);
    }

    #[test]
    fn accepted_and_rejected_event_coords_carry_the_typed_v1_grammar() {
        let accepted = accepted_event_coords().unwrap();
        assert_eq!(accepted.producer.as_str(), "identity");
        assert_eq!(accepted.aggregate.as_str(), "service_scope");
        assert_eq!(accepted.fact.as_str(), "accepted");
        assert_eq!(accepted.version, 1);

        let rejected = rejected_event_coords().unwrap();
        assert_eq!(rejected.fact.as_str(), "rejected");
    }

    #[test]
    fn command_type_is_the_aggregate_verb_pair() {
        assert_eq!(command_type(), "service_scope.declare");
    }

    #[test]
    fn event_type_carries_the_aggregate_prefix() {
        assert_eq!(event_type(ACCEPTED), "service_scope.accepted");
        assert_eq!(event_type(REJECTED), "service_scope.rejected");
    }
}
