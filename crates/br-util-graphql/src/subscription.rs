//! [`SubscriptionPayload`] — the collaborative-pure push shape.
//!
//! The doctrine (R1 + R2): the client fetches one snapshot, then folds the
//! subscription stream into local state — it never refetches on a signal. So a
//! push must carry everything the client needs to update without a round-trip:
//!
//! 1. the **`event`** that happened (the near-unfiltered domain-event union, the
//!    same data the domain event carries — a dropped field is a bug);
//! 2. the **fresh `entity`** the event produced (so the client folds new state
//!    directly, never re-queries);
//! 3. the **recalculated `affordances`** for *this subscriber* (so controls stay
//!    live — what *they* may now do, computed in the domain, not the client).
//!
//! Generic over the event union `E` and the entity `T`, both
//! `async_graphql::OutputType`. The GraphQL name is woven from the entity
//! (`{Entity}SubscriptionPayload`) so two subdomains' payloads do not collide in
//! one schema (the same per-node-naming idiom as [`Connection`](crate::Connection);
//! pinned by `tests/schema_mounting.rs`).
//!
//! A push that only signalled "something changed" (forcing a refetch) is exactly
//! what this type prevents: it always carries the new state.

use std::borrow::Cow;

use async_graphql::{OutputType, SimpleObject, TypeName};

use crate::affordance::Affordance;

/// One collaborative-pure push: what happened (`event`), the resulting state
/// (`entity`), and the subscriber's recalculated `affordances`.
#[derive(SimpleObject)]
#[graphql(name_type)]
pub struct SubscriptionPayload<E: OutputType, T: OutputType> {
    /// The domain event that occurred (the subscription event union).
    pub event: E,
    /// The fresh entity the event produced — the client folds this directly,
    /// never re-querying.
    pub entity: T,
    /// The affordances recalculated for this subscriber after the event.
    pub affordances: Vec<Affordance>,
}

impl<E: OutputType, T: OutputType> TypeName for SubscriptionPayload<E, T> {
    fn type_name() -> Cow<'static, str> {
        Cow::Owned(format!("{}SubscriptionPayload", T::type_name()))
    }
}

impl<E: OutputType, T: OutputType> SubscriptionPayload<E, T> {
    /// Assemble a push from the event, the fresh entity, and the affordances
    /// recomputed for the subscriber.
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

    // Given an event + fresh entity + affordances, Then the payload carries all
    // three (the push needs no refetch to be applied).
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
