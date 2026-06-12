use std::borrow::Cow;

use async_graphql::{OutputType, SimpleObject, TypeName};

use crate::affordance::Affordance;

#[derive(SimpleObject)]
#[graphql(name_type)]
pub struct SubscriptionPayload<E: OutputType, T: OutputType> {
    pub event: E,
    pub entity: T,
    pub affordances: Vec<Affordance>,
}

impl<E: OutputType, T: OutputType> TypeName for SubscriptionPayload<E, T> {
    fn type_name() -> Cow<'static, str> {
        Cow::Owned(format!("{}SubscriptionPayload", T::type_name()))
    }
}

impl<E: OutputType, T: OutputType> SubscriptionPayload<E, T> {
    pub fn new(event: E, entity: T, affordances: Vec<Affordance>) -> Self {
        Self {
            event,
            entity,
            affordances,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_graphql::SimpleObject;

    #[derive(SimpleObject)]
    struct Renamed {
        new_name: String,
    }

    #[derive(SimpleObject)]
    struct Doc {
        id: String,
        name: String,
    }

    #[test]
    fn payload_carries_event_entity_and_affordances() {
        let payload = SubscriptionPayload::new(
            Renamed {
                new_name: "Acme".into(),
            },
            Doc {
                id: "doc-1".into(),
                name: "Acme".into(),
            },
            vec![
                Affordance::allow("rename"),
                Affordance::block("delete", "locked"),
            ],
        );
        assert_eq!(payload.event.new_name, "Acme");
        assert_eq!(payload.entity.name, "Acme");
        assert_eq!(payload.affordances.len(), 2);
        assert_eq!(
            payload.affordances[1].reason_code.as_deref(),
            Some("locked")
        );
    }
}
