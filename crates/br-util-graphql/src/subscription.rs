use std::borrow::Cow;
use std::marker::PhantomData;

use async_graphql::{OutputType, SimpleObject, TypeName};

use crate::affordance::Affordance;

pub trait PayloadName {
    const NAME: &'static str;
}

#[derive(SimpleObject)]
#[graphql(name_type)]
pub struct SubscriptionPayload<N: PayloadName, E: OutputType, T: OutputType> {
    pub event: E,
    pub entity: T,
    pub affordances: Vec<Affordance>,
    #[graphql(skip)]
    _name: PhantomData<fn() -> N>,
}

impl<N: PayloadName, E: OutputType, T: OutputType> TypeName for SubscriptionPayload<N, E, T> {
    fn type_name() -> Cow<'static, str> {
        Cow::Borrowed(N::NAME)
    }
}

impl<N: PayloadName, E: OutputType, T: OutputType> SubscriptionPayload<N, E, T> {
    pub fn new(event: E, entity: T, affordances: Vec<Affordance>) -> Self {
        Self {
            event,
            entity,
            affordances,
            _name: PhantomData,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_graphql::SimpleObject;

    struct DocChangeName;
    impl PayloadName for DocChangeName {
        const NAME: &'static str = "DocChangeSubscriptionPayload";
    }

    struct DocAuditName;
    impl PayloadName for DocAuditName {
        const NAME: &'static str = "DocAuditSubscriptionPayload";
    }

    #[derive(SimpleObject)]
    struct Renamed {
        new_name: String,
    }

    #[derive(SimpleObject)]
    struct Audited {
        at: String,
    }

    #[derive(SimpleObject)]
    struct Doc {
        id: String,
        name: String,
    }

    fn is_valid_gql_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
            && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
    }

    fn payload_type_name<N: PayloadName, E: OutputType, T: OutputType>() -> Cow<'static, str> {
        <SubscriptionPayload<N, E, T> as TypeName>::type_name()
    }

    #[test]
    fn type_name_is_the_caller_supplied_name() {
        let name = payload_type_name::<DocChangeName, Renamed, Doc>();
        assert!(is_valid_gql_identifier(&name));
        assert_eq!(name, "DocChangeSubscriptionPayload");
    }

    #[test]
    fn same_entity_with_different_events_never_collides() {
        let change = payload_type_name::<DocChangeName, Renamed, Doc>();
        let audit = payload_type_name::<DocAuditName, Audited, Doc>();
        assert_ne!(change, audit);
    }

    #[test]
    fn list_entity_uses_the_caller_supplied_name() {
        let name = payload_type_name::<DocChangeName, Renamed, Vec<Doc>>();
        assert_eq!(name, "DocChangeSubscriptionPayload");
    }

    #[test]
    fn payload_carries_event_entity_and_affordances() {
        let payload = SubscriptionPayload::<DocChangeName, _, _>::new(
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
