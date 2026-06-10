//! Tuning for the boot-time scope-declaration handshake.

use std::time::Duration;

use br_core_integration::AwaiterConfig;

/// Configuration for [`declare_scopes`](crate::declare_scopes).
///
/// The two knobs a consumer wires from Helm are `enabled` (the per-project
/// opt-out) and `stream_name` (the pre-declared JetStream stream capturing the
/// `identity.evt.service_scope.*` confirmation subjects). The timing knobs have
/// sensible defaults; override only for an unusual deployment.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ScopeDeclarationConfig {
    /// Whether to perform the handshake at all (the per-project opt-out).
    ///
    /// `false` skips it entirely — no awaiter, no publish — and
    /// [`declare_scopes`](crate::declare_scopes) returns
    /// [`Disabled`](crate::ScopeDeclarationOutcome::Disabled) after setting the
    /// readiness gate **UP** (the consumer wired the gate expecting the helper
    /// to drive it, so the helper leaves it in a serve-traffic state). This is
    /// the disabled mode, **distinct** from the intrinsic *scopeless* case: a
    /// service that owns no scopes does not call this helper at all. Wire this
    /// from Helm.
    pub enabled: bool,

    /// Name of the **pre-declared** JetStream stream that captures the two
    /// confirmation subjects (`identity.evt.service_scope.accepted.v1` and
    /// `…rejected.v1`). The awaiter binds it by name and **fails loud** if it is
    /// missing — the helper never creates a stream.
    pub stream_name: String,

    /// Per-wait deadline before re-publishing the declare command. On each
    /// timeout the helper re-publishes (same `correlation_id`) and waits again —
    /// indefinitely, because Identity may be down and the readiness gate keeps
    /// the pod out of rotation meanwhile (accepted coupling). Default:
    /// [`DEFAULT_WAIT_TIMEOUT`](Self::DEFAULT_WAIT_TIMEOUT) (10s).
    pub wait_timeout: Duration,

    /// Tuning for the awaiter's ephemeral consumer. The default 300s
    /// `inactive_threshold` is far above any sane `wait_timeout`, so the awaiter
    /// stays armed across the re-publish gap with no extra configuration; only a
    /// deployment with an unusually long gap between waits needs to raise it.
    pub awaiter: AwaiterConfig,
}

impl ScopeDeclarationConfig {
    /// Default per-wait deadline before a re-publish: 10s. Long enough to absorb
    /// normal broker + Identity latency, short enough that a single missed
    /// confirmation is re-driven promptly.
    pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

    /// An **enabled** config for `stream_name`, with the default timing knobs
    /// (10s wait, 300s awaiter `inactive_threshold`).
    pub fn enabled(stream_name: impl Into<String>) -> Self {
        Self {
            enabled: true,
            stream_name: stream_name.into(),
            wait_timeout: Self::DEFAULT_WAIT_TIMEOUT,
            awaiter: AwaiterConfig::default(),
        }
    }

    /// A **disabled** config (the per-project opt-out). `stream_name` is unused
    /// in this mode — the handshake is skipped — but is kept so a consumer can
    /// flip `enabled` from one config value.
    pub fn disabled(stream_name: impl Into<String>) -> Self {
        Self {
            enabled: false,
            ..Self::enabled(stream_name)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_uses_default_timings() {
        let c = ScopeDeclarationConfig::enabled("IDENTITY");
        assert!(c.enabled);
        assert_eq!(c.stream_name, "IDENTITY");
        assert_eq!(c.wait_timeout, ScopeDeclarationConfig::DEFAULT_WAIT_TIMEOUT);
        assert_eq!(c.awaiter, AwaiterConfig::default());
    }

    #[test]
    fn disabled_flips_only_the_flag() {
        let c = ScopeDeclarationConfig::disabled("IDENTITY");
        assert!(!c.enabled);
        assert_eq!(c.stream_name, "IDENTITY");
    }
}
