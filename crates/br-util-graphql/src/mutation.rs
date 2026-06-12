//! [`MutationResult`] — the only thing a BR mutation returns on success: an ack.
//!
//! The doctrine (R1, collaborative-pure): **a mutation is a command; it returns
//! a verdict, never the mutated state.** Success → this ack; the new state
//! reaches every client through the event stream, so two clients can never
//! diverge by one of them applying a mutation's return value directly. Failure
//! without state change → an [`EdgeError`](crate::EdgeError), not this type.
//!
//! This type locks the rule into the schema: there is no field on it to smuggle
//! a DTO back. The sole estate-wide exception — a one-shot secret (PAT / API key
//! / initial password) returned exactly once in the mutation response — is a
//! *different*, deliberately-named return type a service defines for that
//! mutation; it is never bolted onto this ack.

use async_graphql::SimpleObject;

/// The success ack of a state-changing mutation. Carries no domain state by
/// design — state arrives via the event stream / subscription.
#[derive(SimpleObject, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationResult {
    /// Always `true` — a mutation that did not succeed returns an
    /// [`EdgeError`](crate::EdgeError) instead of this ack. The field exists so
    /// the GraphQL type is non-empty and the client has a positive signal.
    pub success: bool,
}

impl MutationResult {
    /// The success ack. The only constructor — there is no failure variant of
    /// this type (failure is an error, not a result).
    ///
    /// No `Default` impl on purpose: a positive ack must be **decided** by a
    /// handler that reached success, never produced silently by a
    /// `..Default::default()` / `MutationResult::default()` (which would fabricate
    /// a `success: true` no one asserted). Always call `ok()` at the success site.
    pub fn ok() -> Self {
        Self { success: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The ack is positive and carries no state — the type has exactly one
    // boolean field, so a DTO cannot be smuggled through it.
    #[test]
    fn ack_is_success_only() {
        assert!(MutationResult::ok().success);
    }
}
