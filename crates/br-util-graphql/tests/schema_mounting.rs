#![cfg(feature = "graphql")]

use async_graphql::{EmptyMutation, EmptySubscription, Object, Schema, SimpleObject, Union, Value};
use br_core_values::{Currency, Money};
use br_util_graphql::values::{GqlMoney, GqlMoneyInput};
use br_util_graphql::{Affordance, Connection, Edge, MutationResult, SubscriptionPayload};

#[derive(SimpleObject, Clone)]
struct Doc {
    id: String,
    name: String,
}

#[derive(SimpleObject, Clone)]
struct Tag {
    label: String,
}

#[derive(SimpleObject, Clone)]
struct DocRenamed {
    new_name: String,
}

#[derive(Union, Clone)]
enum DocEvent {
    Renamed(DocRenamed),
}

struct Query;

#[Object]
impl Query {
    async fn docs(&self) -> Connection<Doc> {
        Connection::forward(
            vec![
                Edge::new(
                    Doc {
                        id: "1".into(),
                        name: "Alpha".into(),
                    },
                    "cursor-1",
                ),
                Edge::new(
                    Doc {
                        id: "2".into(),
                        name: "Beta".into(),
                    },
                    "cursor-2",
                ),
            ],
            true,
        )
    }

    async fn last_change(&self) -> SubscriptionPayload<DocEvent, Doc> {
        SubscriptionPayload::new(
            DocEvent::Renamed(DocRenamed {
                new_name: "Beta".into(),
            }),
            Doc {
                id: "2".into(),
                name: "Beta".into(),
            },
            vec![
                Affordance::allow("rename"),
                Affordance::block("delete", "locked").with_param("min_members", "1"),
            ],
        )
    }

    async fn tags(&self) -> Connection<Tag> {
        Connection::forward(vec![Edge::new(Tag { label: "x".into() }, "tc-1")], false)
    }

    async fn ack(&self) -> MutationResult {
        MutationResult::ok()
    }

    async fn balance(&self) -> GqlMoney {
        let big = i64::from(i32::MAX) * 1_000_000;
        GqlMoney::from(&Money::new(big, Currency::new("EUR").unwrap()))
    }

    async fn echo_money(&self, input: GqlMoneyInput) -> GqlMoney {
        let money = Money::try_from(input).unwrap();
        GqlMoney::from(&money)
    }
}

fn schema() -> Schema<Query, EmptyMutation, EmptySubscription> {
    Schema::new(Query, EmptyMutation, EmptySubscription)
}

#[tokio::test]
async fn connection_registers_and_resolves() {
    let query = r#"
        { docs {
            edges { node { id name } cursor }
            pageInfo { hasNextPage hasPreviousPage startCursor endCursor }
        } }
    "#;
    let response = schema().execute(query).await;
    assert!(response.errors.is_empty(), "errors: {:?}", response.errors);

    let data = response.data.into_json().unwrap();
    let edges = &data["docs"]["edges"];
    assert_eq!(edges[0]["node"]["name"], "Alpha");
    assert_eq!(edges[0]["cursor"], "cursor-1");
    assert_eq!(edges[1]["cursor"], "cursor-2");
    assert_eq!(data["docs"]["pageInfo"]["hasNextPage"], true);
    assert_eq!(data["docs"]["pageInfo"]["hasPreviousPage"], false);
    assert_eq!(data["docs"]["pageInfo"]["startCursor"], "cursor-1");
    assert_eq!(data["docs"]["pageInfo"]["endCursor"], "cursor-2");
}

#[tokio::test]
async fn subscription_payload_registers_and_resolves() {
    let query = r#"
        { lastChange {
            event { __typename ... on DocRenamed { newName } }
            entity { id name }
            affordances { action allowed reasonCode params }
        } }
    "#;
    let response = schema().execute(query).await;
    assert!(response.errors.is_empty(), "errors: {:?}", response.errors);

    let data = response.data.into_json().unwrap();
    assert_eq!(data["lastChange"]["event"]["__typename"], "DocRenamed");
    assert_eq!(data["lastChange"]["event"]["newName"], "Beta");
    assert_eq!(data["lastChange"]["entity"]["name"], "Beta");
    let affordances = data["lastChange"]["affordances"].as_array().unwrap();
    assert_eq!(affordances.len(), 2);
    assert_eq!(affordances[0]["action"], "rename");
    assert_eq!(affordances[0]["allowed"], true);
    assert_eq!(
        affordances[0]["reasonCode"],
        Value::Null.into_json().unwrap()
    );
    assert_eq!(affordances[0]["params"], Value::Null.into_json().unwrap());
    assert_eq!(affordances[1]["allowed"], false);
    assert_eq!(affordances[1]["reasonCode"], "locked");
    assert_eq!(affordances[1]["params"]["min_members"], "1");
}

#[tokio::test]
async fn money_amount_resolves_as_full_i64_decimal_string() {
    let big = i64::from(i32::MAX) * 1_000_000;
    let response = schema().execute("{ balance { amount currency } }").await;
    assert!(response.errors.is_empty(), "errors: {:?}", response.errors);

    let data = response.data.into_json().unwrap();
    assert_eq!(data["balance"]["amount"], big.to_string());
    assert_eq!(data["balance"]["currency"], "EUR");
}

#[tokio::test]
async fn money_input_round_trips_large_i64() {
    let big = i64::from(i32::MAX) * 1_000_000;
    let query = format!(
        r#"{{ echoMoney(input: {{ amount: "{big}", currency: "USD" }}) {{ amount currency }} }}"#
    );
    let response = schema().execute(query).await;
    assert!(response.errors.is_empty(), "errors: {:?}", response.errors);

    let data = response.data.into_json().unwrap();
    assert_eq!(data["echoMoney"]["amount"], big.to_string());
    assert_eq!(data["echoMoney"]["currency"], "USD");
}

#[tokio::test]
async fn money_input_refuses_unparsable_amount() {
    let query = r#"{ echoMoney(input: { amount: "not-a-number", currency: "USD" }) { amount } }"#;
    let response = schema().execute(query).await;
    assert!(
        response
            .errors
            .iter()
            .any(|e| e.message.contains("MONEY_OUT_OF_RANGE")),
        "expected MONEY_OUT_OF_RANGE, got: {:?}",
        response.errors
    );
}

#[tokio::test]
async fn mutation_result_registers_and_resolves() {
    let response = schema().execute("{ ack { success } }").await;
    assert!(response.errors.is_empty(), "errors: {:?}", response.errors);
    let data = response.data.into_json().unwrap();
    assert_eq!(data["ack"]["success"], true);
}

#[tokio::test]
async fn sdl_exposes_the_generic_types() {
    let sdl = schema().sdl();
    assert!(
        sdl.contains("type DocConnection"),
        "missing DocConnection:\n{sdl}"
    );
    assert!(sdl.contains("type DocEdge"), "missing DocEdge:\n{sdl}");
    assert!(sdl.contains("type PageInfo"), "missing PageInfo:\n{sdl}");
    assert!(
        sdl.contains("type Affordance"),
        "missing Affordance:\n{sdl}"
    );
    assert!(
        sdl.contains("params: JSON"),
        "Affordance.params must render as the JSON scalar:\n{sdl}"
    );
    assert!(
        sdl.contains("type DocSubscriptionPayload"),
        "missing DocSubscriptionPayload:\n{sdl}"
    );
    assert!(
        sdl.contains("type TagConnection"),
        "missing TagConnection — generic naming collided:\n{sdl}"
    );
    assert!(
        sdl.contains("scalar MoneyAmount"),
        "missing the MoneyAmount scalar — money must not ride the 32-bit Int:\n{sdl}"
    );
    assert!(
        sdl.contains("amount: MoneyAmount!"),
        "GqlMoney.amount must be the MoneyAmount scalar, not Int:\n{sdl}"
    );
}
