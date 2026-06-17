use crate::coords::newtypes::{Aggregate, Bc, PastFact, Verb};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCoords {
    pub receiver: Bc,
    pub aggregate: Aggregate,
    pub verb: Verb,
    pub version: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCoords {
    pub producer: Bc,
    pub aggregate: Aggregate,
    pub fact: PastFact,
    pub version: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_coords_holds_its_segments() {
        let coords = CommandCoords {
            receiver: Bc::new("identity").unwrap(),
            aggregate: Aggregate::new("service_scope").unwrap(),
            verb: Verb::new("declare").unwrap(),
            version: 1,
        };
        assert_eq!(coords.receiver.as_str(), "identity");
        assert_eq!(coords.aggregate.as_str(), "service_scope");
        assert_eq!(coords.verb.as_str(), "declare");
        assert_eq!(coords.version, 1);
    }

    #[test]
    fn event_coords_holds_its_segments() {
        let coords = EventCoords {
            producer: Bc::new("identity").unwrap(),
            aggregate: Aggregate::new("service_scope").unwrap(),
            fact: PastFact::new("accepted").unwrap(),
            version: 1,
        };
        assert_eq!(coords.producer.as_str(), "identity");
        assert_eq!(coords.fact.as_str(), "accepted");
        assert_eq!(coords.version, 1);
    }
}
