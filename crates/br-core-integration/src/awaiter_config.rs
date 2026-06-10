//! Tuning for the [`CorrelatedAwaiter`](crate::CorrelatedAwaiter)'s ephemeral
//! consumer.

use std::time::Duration;

/// Tuning for the ephemeral awaiter consumer.
///
/// The only knob today is `inactive_threshold`: how long the server keeps the
/// ephemeral consumer alive across periods where nothing pulls it (i.e. between
/// waits — see the [awaiter module docs](crate::awaiter)). Build with
/// [`AwaiterConfig::default`] (300s) or set it explicitly for a workload whose
/// gap between a timed-out wait and the next re-publish can exceed that.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AwaiterConfig {
    /// How long the server keeps the ephemeral consumer after the last pull
    /// before reaping it. Must comfortably exceed the longest expected idle gap
    /// between waits, or the awaiter is reaped mid-protocol and the next wait
    /// fails loud with
    /// [`ConsumeErrorKind::ConsumerGone`](crate::ConsumeErrorKind::ConsumerGone).
    pub inactive_threshold: Duration,
}

impl AwaiterConfig {
    /// Default `inactive_threshold`: 300s. Generous on purpose — the awaiter
    /// must survive a caller backing off and re-publishing between waits without
    /// being reaped server-side.
    pub const DEFAULT_INACTIVE_THRESHOLD: Duration = Duration::from_secs(300);
}

impl Default for AwaiterConfig {
    fn default() -> Self {
        Self {
            inactive_threshold: Self::DEFAULT_INACTIVE_THRESHOLD,
        }
    }
}
