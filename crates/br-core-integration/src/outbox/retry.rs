//! The relay's two **pure** publish-failure policies — no I/O, no sqlx, no NATS,
//! so both are unit-tested as specs.
//!
//! 1. [`FailureClass`] / [`classify_failure`] — is a publish failure a
//!    **structural** declaration fault (the target stream is not declared, fail
//!    loud, do not burn the row's attempt budget) or a **transient** transport
//!    fault (retry within the budget)?
//! 2. [`retry_backoff`] — how long to wait before re-draining after a transient
//!    failure, derived from the row's attempt count, exponential and capped.

use std::time::Duration;

use crate::error::{IntegrationError, PublishErrorKind};

/// How a publish failure is classified for the relay's purposes.
///
/// `#[non_exhaustive]`: a future class is an additive change; match with a
/// wildcard arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FailureClass {
    /// The target JetStream object is not declared (`NoStream`) — an
    /// infra-declaration fault, not a delivery attempt against the row. The row
    /// stays `Pending` **without** consuming an attempt, and the relay flips its
    /// health to `Degraded` so the consuming service can fail its readiness gate.
    Structural,
    /// A transport-level fault (timeout, broken pipe, broker unreachable). It
    /// counts as a delivery attempt against `max_attempts` and drives the
    /// chained retry deadline.
    Transient,
}

/// Classify a publish [`IntegrationError`]: a `NoStream` publish failure is a
/// [`Structural`](FailureClass::Structural) declaration fault; everything else —
/// a `Timeout`, an `Other` transport fault, or a non-publish error such as a
/// serialization failure — is [`Transient`](FailureClass::Transient).
///
/// `NoStream` is the only structural case the **publish** path can surface: a
/// JetStream publish to a subject no stream captures answers "no stream found",
/// which means the declared stream is missing (the same fail-loud signal
/// `verify_consumer` and the consumer shapes raise for `NoStream` / `NoConsumer`)
/// — not a blip to burn retries on. A `Timeout` is genuinely transient, so it is
/// *not* structural even though a publisher classifies it distinctly.
pub fn classify_failure(err: &IntegrationError) -> FailureClass {
    match err {
        IntegrationError::Publish {
            kind: PublishErrorKind::NoStream,
            ..
        } => FailureClass::Structural,
        _ => FailureClass::Transient,
    }
}

/// The base unit of the exponential retry backoff (the wait after the first
/// transient failure). Doubles per recorded attempt, capped at [`RETRY_BACKOFF_MAX`].
pub const RETRY_BACKOFF_BASE: Duration = Duration::from_millis(500);

/// The ceiling on the retry backoff — a persistently-failing transient row
/// re-drains at most this often, so the chained retry never degenerates into a
/// busy interval and never stretches unboundedly.
pub const RETRY_BACKOFF_MAX: Duration = Duration::from_secs(30);

/// The delay before the next chained re-drain after a transient failure left a
/// row `Pending`, as a function of the attempts already recorded on that row.
///
/// Exponential (`base * 2^(attempts-1)`), capped at [`RETRY_BACKOFF_MAX`]. The
/// first transient failure (one attempt recorded) waits [`RETRY_BACKOFF_BASE`];
/// each subsequent attempt doubles the wait until the cap. This is the **chained
/// one-shot** backoff — it is armed off an actual failure, never a blind clock;
/// when no row is owed a retry the deadline arm is absent from the relay's
/// `select!` and it parks at zero CPU.
///
/// `attempts` is clamped to ≥1 (a row left `Pending` by a transient failure has
/// recorded at least one attempt); the doubling is saturating so a large attempt
/// count cannot overflow — it simply pins at the cap.
pub fn retry_backoff(attempts: u32) -> Duration {
    let attempts = attempts.max(1);
    // base * 2^(attempts-1), saturating: a huge shift pins at the cap.
    let shift = attempts - 1;
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let millis = RETRY_BACKOFF_BASE
        .as_millis()
        .saturating_mul(u128::from(factor));
    let capped = millis.min(RETRY_BACKOFF_MAX.as_millis());
    // capped ≤ RETRY_BACKOFF_MAX (30_000 ms) — always fits in u64.
    Duration::from_millis(capped as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    // GIVEN a NoStream publish failure WHEN classified THEN it is Structural
    // (an undeclared stream is a declaration fault, not a delivery attempt).
    #[test]
    fn no_stream_publish_failure_is_structural() {
        let err = IntegrationError::Publish {
            kind: PublishErrorKind::NoStream,
            detail: "no stream found for given subject".to_string(),
        };
        assert_eq!(classify_failure(&err), FailureClass::Structural);
    }

    // GIVEN a timeout publish failure WHEN classified THEN it is Transient
    // (a timeout is a transport blip, retried against the attempt budget).
    #[test]
    fn timeout_publish_failure_is_transient() {
        let err = IntegrationError::Publish {
            kind: PublishErrorKind::Timeout,
            detail: "broker did not ack in time".to_string(),
        };
        assert_eq!(classify_failure(&err), FailureClass::Transient);
    }

    // GIVEN an ambiguous (Other) publish failure WHEN classified THEN it is Transient
    #[test]
    fn other_publish_failure_is_transient() {
        let err = IntegrationError::Publish {
            kind: PublishErrorKind::Other,
            detail: "connection reset".to_string(),
        };
        assert_eq!(classify_failure(&err), FailureClass::Transient);
    }

    // GIVEN a non-publish error (serialization) WHEN classified THEN it is Transient
    // (it is not a missing-declaration fault — it never reached the transport).
    #[test]
    fn non_publish_error_is_transient() {
        let bad: Result<serde_json::Value, _> = serde_json::from_str("{ not json");
        let err: IntegrationError = bad.unwrap_err().into();
        assert_eq!(classify_failure(&err), FailureClass::Transient);
    }

    // GIVEN the first transient failure WHEN the backoff is computed
    // THEN it is the base delay
    #[test]
    fn first_attempt_waits_the_base() {
        assert_eq!(retry_backoff(1), RETRY_BACKOFF_BASE);
    }

    // GIVEN a zero attempt count (meaningless) WHEN computed THEN it clamps to the base
    #[test]
    fn zero_attempts_is_clamped_to_the_base() {
        assert_eq!(retry_backoff(0), RETRY_BACKOFF_BASE);
    }

    // GIVEN successive attempts WHEN the backoff is computed THEN it doubles
    #[test]
    fn backoff_doubles_per_attempt() {
        assert_eq!(retry_backoff(1), Duration::from_millis(500));
        assert_eq!(retry_backoff(2), Duration::from_millis(1000));
        assert_eq!(retry_backoff(3), Duration::from_millis(2000));
        assert_eq!(retry_backoff(4), Duration::from_millis(4000));
    }

    // GIVEN many attempts WHEN the backoff is computed THEN it pins at the cap,
    // never overflowing on a large shift.
    #[test]
    fn backoff_caps_and_never_overflows() {
        assert_eq!(retry_backoff(20), RETRY_BACKOFF_MAX);
        assert_eq!(retry_backoff(u32::MAX), RETRY_BACKOFF_MAX);
    }
}
