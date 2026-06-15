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
        Cow::Owned(format!(
            "{}SubscriptionPayload",
            identifier_from_gql_type(&T::type_name())
        ))
    }
}

fn identifier_from_gql_type(gql_type: &str) -> String {
    let inner = gql_type.trim_end_matches('!');
    match inner.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        Some(element) => format!("{}List", identifier_from_gql_type(element)),
        None => inner.to_owned(),
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

    fn is_valid_gql_identifier(name: &str) -> bool {
        let mut chars = name.chars();
        matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
            && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
    }

    fn payload_type_name<E: OutputType, T: OutputType>() -> Cow<'static, str> {
        <SubscriptionPayload<E, T> as TypeName>::type_name()
    }

    #[test]
    fn list_entity_payload_has_a_valid_graphql_type_name() {
        let name = payload_type_name::<Renamed, Vec<Doc>>();
        assert!(
            is_valid_gql_identifier(&name),
            "expected a valid GraphQL identifier, got `{name}`"
        );
    }

    #[test]
    fn scalar_entity_payload_keeps_the_plain_suffix_name() {
        let name = payload_type_name::<Renamed, Doc>();
        assert_eq!(name, "DocSubscriptionPayload");
    }

    #[test]
    fn scalar_and_list_payloads_of_the_same_entity_never_collide() {
        let scalar = payload_type_name::<Renamed, Doc>();
        let list = payload_type_name::<Renamed, Vec<Doc>>();
        assert_ne!(scalar, list);
        assert_eq!(list, "DocListSubscriptionPayload");
    }

    #[test]
    fn nested_list_entity_encodes_each_level_distinctly() {
        let single = payload_type_name::<Renamed, Vec<Doc>>();
        let nested = payload_type_name::<Renamed, Vec<Vec<Doc>>>();
        assert!(is_valid_gql_identifier(&nested));
        assert_ne!(single, nested);
        assert_eq!(nested, "DocListListSubscriptionPayload");
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
