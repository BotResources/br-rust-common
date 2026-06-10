//! The terminal outcome of the boot-time handshake.

use br_core_scope::ServiceScopesRejected;

/// What [`declare_scopes`](crate::declare_scopes) resolved to.
///
/// The helper drives the readiness gate as a side effect (UP on `Accepted` /
/// `Disabled`, DOWN on `Rejected`) and **also** returns the outcome so the
/// caller can decide what to do with the process — keep it alive out of rotation
/// (the recommended default for a `Rejected`, leaving the gate DOWN) or exit.
///
/// There is no timeout variant: a timed-out wait is not terminal — the helper
/// re-publishes (same `correlation_id`) and keeps waiting indefinitely, with the
/// gate held DOWN, until a confirmation arrives (Identity may be down; that
/// coupling is accepted by design).
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future outcome is an
/// additive change.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ScopeDeclarationOutcome {
    /// Identity accepted the declaration. The readiness gate was set **UP**.
    Accepted,
    /// Identity rejected the declaration. The readiness gate was set **DOWN**
    /// and the structured reason was logged (`tracing::error`, codes not prose).
    /// Rejection is deterministic — re-declaring would not change it — so the
    /// helper does **not** retry; it returns the reason for the caller to act on.
    Rejected(ServiceScopesRejected),
    /// The handshake was skipped (`enabled = false`, the per-project opt-out).
    /// The readiness gate was set **UP** — the consumer wired the gate expecting
    /// the helper to drive it, so the helper leaves it serving traffic.
    Disabled,
}

impl ScopeDeclarationOutcome {
    /// Whether the service ended up gated **ready** to serve traffic
    /// ([`Accepted`](Self::Accepted) or [`Disabled`](Self::Disabled)).
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Accepted | Self::Disabled)
    }
}
