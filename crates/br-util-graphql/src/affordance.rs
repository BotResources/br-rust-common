//! [`Affordance`] — the first-class affordance shape every BR frontend renders.
//!
//! The doctrine: **all logic and affordances live in the domain; the frontend
//! is dumb.** For every action the domain computes `{ action, allowed,
//! reason_code }` and projects it — in the read snapshot *and* re-emitted on
//! every state-changing event — so the client's controls (enabled / disabled,
//! with a reason) stay live without a line of local logic. The client never
//! re-derives "can I click this?"; it renders what the domain projected.
//!
//! `reason_code` is a **stable code, never a sentence** (codes-not-language): an
//! English string here would break the JP/FR UI. There is no `allow`-with-reason
//! constructor — an allowed affordance has no reason — and no way to build a
//! blocked affordance without a code, so a silent denial is unrepresentable.

use async_graphql::SimpleObject;

/// One projected affordance: whether `action` is currently allowed, and — when
/// blocked — the stable `reason_code` the frontend maps to localized copy.
///
/// Build it with [`Affordance::allow`] or [`Affordance::block`]; the constructors
/// make a blocked-without-reason value impossible to express.
#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct Affordance {
    /// The action this affordance governs (a stable action key, e.g. `rename`).
    pub action: String,
    /// Whether the action is currently permitted.
    pub allowed: bool,
    /// When blocked, the stable reason code (never UI prose). `None` iff
    /// `allowed` is `true`.
    pub reason_code: Option<String>,
}

impl Affordance {
    /// An allowed affordance — no reason (the action is available).
    pub fn allow(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            allowed: true,
            reason_code: None,
        }
    }

    /// A blocked affordance carrying its stable reason code. A blocked
    /// affordance **must** state why — there is no silent-denial constructor.
    pub fn block(action: impl Into<String>, reason_code: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            allowed: false,
            reason_code: Some(reason_code.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Given an allowed action, Then it carries no reason.
    #[test]
    fn allow_has_no_reason() {
        let a = Affordance::allow("rename");
        assert_eq!(a.action, "rename");
        assert!(a.allowed);
        assert_eq!(a.reason_code, None);
    }

    // Given a blocked action, Then it carries the stable reason code (a denial
    // is never silent).
    #[test]
    fn block_carries_its_reason_code() {
        let a = Affordance::block("rename", "title_generating");
        assert!(!a.allowed);
        assert_eq!(a.reason_code.as_deref(), Some("title_generating"));
    }

    // codes-not-language: a reason code is a key, not a sentence.
    #[test]
    fn reason_code_is_a_key_not_prose() {
        let a = Affordance::block("delete", "system_group_protected");
        let reason = a.reason_code.unwrap();
        assert!(
            !reason.contains(' '),
            "reason looks like a sentence: {reason}"
        );
    }
}
