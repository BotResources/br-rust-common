use br_core_integration::{CommandCoords, EventCoords};

pub(crate) const INTEGRATION_PREFIX: &str = "integration";
pub(crate) const CMD_TOKEN: &str = "cmd";
pub(crate) const EVT_TOKEN: &str = "evt";

pub(crate) trait IntegrationSubject {
    fn subject(&self) -> String;
}

pub fn command_subject(coords: &CommandCoords) -> String {
    coords.subject()
}

pub fn event_subject(coords: &EventCoords) -> String {
    coords.subject()
}

impl IntegrationSubject for CommandCoords {
    fn subject(&self) -> String {
        format!(
            "{INTEGRATION_PREFIX}.{CMD_TOKEN}.{}.{}.{}.v{}",
            self.receiver.as_str(),
            self.aggregate.as_str(),
            self.verb.as_str(),
            self.version
        )
    }
}

impl IntegrationSubject for EventCoords {
    fn subject(&self) -> String {
        format!(
            "{INTEGRATION_PREFIX}.{EVT_TOKEN}.{}.{}.{}.v{}",
            self.producer.as_str(),
            self.aggregate.as_str(),
            self.fact.as_str(),
            self.version
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_integration::{Aggregate, Bc, PastFact, Verb};

    fn bc(v: &str) -> Bc {
        Bc::new(v).unwrap()
    }
    fn agg(v: &str) -> Aggregate {
        Aggregate::new(v).unwrap()
    }

    #[test]
    fn command_renders_the_fixed_six_segment_grammar() {
        let coords = CommandCoords {
            receiver: bc("notifier"),
            aggregate: agg("notification"),
            verb: Verb::new("deliver").unwrap(),
            version: 1,
        };
        assert_eq!(
            coords.subject(),
            "integration.cmd.notifier.notification.deliver.v1"
        );
    }

    #[test]
    fn event_renders_the_fixed_six_segment_grammar() {
        let coords = EventCoords {
            producer: bc("identity"),
            aggregate: agg("user"),
            fact: PastFact::new("created").unwrap(),
            version: 2,
        };
        assert_eq!(coords.subject(), "integration.evt.identity.user.created.v2");
    }

    #[test]
    fn prefix_is_always_integration_and_not_caller_choosable() {
        let coords = EventCoords {
            producer: bc("identity"),
            aggregate: agg("group"),
            fact: PastFact::new("renamed").unwrap(),
            version: 1,
        };
        assert!(coords.subject().starts_with("integration.evt."));
    }
}
