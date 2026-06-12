//! Real-seam integration test: the generic edge types must actually **register
//! and resolve in a built `async_graphql::Schema`**, not merely compile.
//!
//! The generic types (`Connection<T>`, `Edge<T>`, `SubscriptionPayload<E, T>`)
//! are the risk: a generic GraphQL object can type-check yet fail to register,
//! silently drop a field, or — the sharp one — collapse two node types onto one
//! fixed type name and collide. Because these types appear in **every consumer's
//! schema** (the lib's public contract), any of those is a lib bug, not a
//! downstream detail. So this test builds a schema mounting each one, executes a
//! real query against it, asserts the resolved shape, and pins the per-node type
//! names in the SDL. This is the kind of real seam (transport + serialization)
//! the doctrine says to cover end-to-end.

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

// A second, different node type — used to prove two connections coexist without
// the name collision a fixed `Connection` type name would cause.
#[derive(SimpleObject, Clone)]
struct Tag {
    label: String,
}

#[derive(SimpleObject, Clone)]
struct DocRenamed {
    new_name: String,
}

// A service's subscription event union, exactly as a consumer would write it.
#[derive(Union, Clone)]
enum DocEvent {
    Renamed(DocRenamed),
}

struct Query;

#[Object]
impl Query {
    // A query returning the generic Connection<Doc> — proves the connection,
    // its edges, the node, the cursor and pageInfo all register and resolve.
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

    // A query returning the generic SubscriptionPayload<DocEvent, Doc> — proves
    // the push shape (event + entity + affordances) registers and resolves.
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
                Affordance::block("delete", "locked"),
            ],
        )
    }

    // A second connection over a different node type — proves both
    // `DocConnection` and `TagConnection` coexist (no fixed-name collision).
    async fn tags(&self) -> Connection<Tag> {
        Connection::forward(vec![Edge::new(Tag { label: "x".into() }, "tc-1")], false)
    }

    // A mutation-style ack mounted on a query field (no real mutation needed to
    // prove the type registers).
    async fn ack(&self) -> MutationResult {
        MutationResult::ok()
    }

    // Projects a large-i64 Money through GqlMoney — proves the MoneyAmount scalar
    // registers and the full amount survives as a decimal string on the wire (no
    // i32 ceiling, no truncation).
    async fn balance(&self) -> GqlMoney {
        let big = i64::from(i32::MAX) * 1_000_000; // far past the old i32 cap
        GqlMoney::from(&Money::new(big, Currency::new("EUR").unwrap()))
    }

    // Accepts a GqlMoneyInput and echoes it back as GqlMoney — proves a money
    // value round-trips through the real schema (string in → i64 → string out)
    // with no loss.
    async fn echo_money(&self, input: GqlMoneyInput) -> GqlMoney {
        let money = Money::try_from(input).unwrap();
        GqlMoney::from(&money)
    }
}

fn schema() -> Schema<Query, EmptyMutation, EmptySubscription> {
    Schema::new(Query, EmptyMutation, EmptySubscription)
}

// Given a schema mounting the generic Connection<Doc>, When the connection is
// queried, Then the edges, nodes, cursors and pageInfo all resolve.
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

// Given a schema mounting the generic SubscriptionPayload, When queried, Then
// the event, the fresh entity and the recalculated affordances all resolve —
// the collaborative-pure push carries everything, no field dropped.
#[tokio::test]
async fn subscription_payload_registers_and_resolves() {
    let query = r#"
        { lastChange {
            event { __typename ... on DocRenamed { newName } }
            entity { id name }
            affordances { action allowed reasonCode }
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
    assert_eq!(affordances[1]["allowed"], false);
    assert_eq!(affordances[1]["reasonCode"], "locked");
}

// Given a schema mounting GqlMoney, When a large-i64 amount is queried, Then it
// resolves as a decimal STRING carrying the full value — no i32 ceiling, no
// truncation through the real async-graphql serialization.
#[tokio::test]
async fn money_amount_resolves_as_full_i64_decimal_string() {
    let big = i64::from(i32::MAX) * 1_000_000;
    let response = schema().execute("{ balance { amount currency } }").await;
    assert!(response.errors.is_empty(), "errors: {:?}", response.errors);

    let data = response.data.into_json().unwrap();
    // The wire form is a string (JS/JSON-precision-safe), equal to the full i64.
    assert_eq!(data["balance"]["amount"], big.to_string());
    assert_eq!(data["balance"]["currency"], "EUR");
}

// Given a GqlMoneyInput with a large-i64 amount string, When echoed back through
// the schema, Then it round-trips exactly (string → i64 → string), proving no
// truncation on either direction.
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

// Given a non-numeric amount string on the input, When parsed by the schema,
// Then the MoneyAmount scalar refuses it with the MONEY_OUT_OF_RANGE key — never
// coerced or truncated.
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

// The MutationResult ack registers and resolves to `{ success: true }`.
#[tokio::test]
async fn mutation_result_registers_and_resolves() {
    let response = schema().execute("{ ack { success } }").await;
    assert!(response.errors.is_empty(), "errors: {:?}", response.errors);
    let data = response.data.into_json().unwrap();
    assert_eq!(data["ack"]["success"], true);
}

// The SDL exports the generic types under their concrete names — proves a
// consumer's codegen sees stable type names (`DocConnection`, `DocEdge`, …).
#[tokio::test]
async fn sdl_exposes_the_generic_types() {
    let sdl = schema().sdl();
    // async-graphql names a generic object `{T}{Wrapper}` — the concrete node
    // name is woven in, so the schema is not a soup of `Connection1` placeholders.
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
        sdl.contains("type DocSubscriptionPayload"),
        "missing DocSubscriptionPayload:\n{sdl}"
    );
    // The collision fix: a second node type yields a distinct connection type,
    // so both coexist in one schema.
    assert!(
        sdl.contains("type TagConnection"),
        "missing TagConnection — generic naming collided:\n{sdl}"
    );
    // The money amount is its own named scalar (a decimal-string i64), not the
    // built-in 32-bit `Int` — proves the wire form is the wide, precision-safe one.
    assert!(
        sdl.contains("scalar MoneyAmount"),
        "missing the MoneyAmount scalar — money must not ride the 32-bit Int:\n{sdl}"
    );
    assert!(
        sdl.contains("amount: MoneyAmount!"),
        "GqlMoney.amount must be the MoneyAmount scalar, not Int:\n{sdl}"
    );
}
