//! The outbox row's lifecycle: an explicit, guarded state machine with a total
//! `&str` ↔ enum mapping.
//!
//! This is the **pure core** of the outbox — no I/O, no sqlx, no NATS — so its
//! transitions are unit-tested as specs and it compiles without the `outbox`
//! feature. The store/relay (feature-gated) drive it; they never set a status
//! field by hand.

/// Lifecycle of a staged outbox row.
///
/// - `Pending` — staged in the caller's transaction, not yet published. The
///   only state a freshly staged row is in; the only state the relay picks up.
/// - `Published` — confirmed on the bus (broker ack received). **Terminal.**
/// - `Failed` — every publish attempt up to the relay's cap failed. Terminal
///   for the automatic relay; an operator may reset it to `Pending` to retry.
///
/// `#[non_exhaustive]`: match with a wildcard arm so a future state is additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutboxStatus {
    /// Staged, awaiting publish. The relay's pick-up state.
    Pending,
    /// Confirmed on the bus. Terminal.
    Published,
    /// Retries exhausted without a broker ack. Terminal for the relay.
    Failed,
}

/// Why a status string from the database could not be mapped to an
/// [`OutboxStatus`]. The mapping is **total** — an unknown value is a typed
/// error, never silently coerced into a variant.
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("unknown outbox status: {0:?}")]
pub struct UnknownOutboxStatus(pub String);

impl OutboxStatus {
    /// The stored representation. Stable wire/DB string, decoupled from the Rust
    /// identifier so a rename here cannot corrupt existing rows.
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Published => "PUBLISHED",
            Self::Failed => "FAILED",
        }
    }

    /// Parse the stored representation back into the enum. Total: an unrecognised
    /// value returns [`UnknownOutboxStatus`] rather than defaulting — a corrupt
    /// or future row fails loud instead of being misread.
    pub fn from_db_str(value: &str) -> Result<Self, UnknownOutboxStatus> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "PUBLISHED" => Ok(Self::Published),
            "FAILED" => Ok(Self::Failed),
            other => Err(UnknownOutboxStatus(other.to_string())),
        }
    }

    /// Whether this is a terminal state the relay will not act on again.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Published | Self::Failed)
    }
}

/// The relay's decision after a publish attempt on a `Pending` row — the pure
/// transition function. It decides; the store applies it. Given the current
/// `attempts` already recorded and the `max_attempts` cap, an attempt's outcome
/// yields the next status and the new attempt count.
///
/// Separating this from any I/O is what makes the retry policy a unit-testable
/// spec: the relay calls [`next_after_attempt`] and writes whatever it returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    /// The status the row moves to.
    pub status: OutboxStatus,
    /// The attempt count to persist (the prior count plus this attempt).
    pub attempts: u32,
}

/// Compute the next [`Transition`] for a `Pending` row after one publish attempt.
///
/// - `succeeded` → [`OutboxStatus::Published`] (terminal), regardless of count.
/// - failed and the new count `>= max_attempts` → [`OutboxStatus::Failed`]
///   (terminal — the relay gives up).
/// - failed and more attempts remain → stays [`OutboxStatus::Pending`] for the
///   next relay pass.
///
/// `max_attempts` is clamped to at least 1: zero attempts is meaningless (a row
/// must be tried at least once before it can fail).
pub fn next_after_attempt(prior_attempts: u32, max_attempts: u32, succeeded: bool) -> Transition {
    let attempts = prior_attempts.saturating_add(1);
    let cap = max_attempts.max(1);
    let status = if succeeded {
        OutboxStatus::Published
    } else if attempts >= cap {
        OutboxStatus::Failed
    } else {
        OutboxStatus::Pending
    };
    Transition { status, attempts }
}

#[cfg(test)]
mod tests {
    use super::*;

    // GIVEN every status WHEN mapped to its db string and back THEN it round-trips
    #[test]
    fn db_str_round_trips_every_status() {
        for status in [
            OutboxStatus::Pending,
            OutboxStatus::Published,
            OutboxStatus::Failed,
        ] {
            let back = OutboxStatus::from_db_str(status.as_db_str()).unwrap();
            assert_eq!(back, status);
        }
    }

    // GIVEN an unknown db string WHEN parsed THEN it is a typed error, never a default
    #[test]
    fn unknown_db_str_is_a_typed_error() {
        let err = OutboxStatus::from_db_str("ARCHIVED").unwrap_err();
        assert_eq!(err, UnknownOutboxStatus("ARCHIVED".to_string()));
    }

    // GIVEN the terminal classification WHEN queried THEN only Pending is non-terminal
    #[test]
    fn only_pending_is_non_terminal() {
        assert!(!OutboxStatus::Pending.is_terminal());
        assert!(OutboxStatus::Published.is_terminal());
        assert!(OutboxStatus::Failed.is_terminal());
    }

    // GIVEN a pending row on its first attempt WHEN the publish succeeds
    // THEN it transitions to Published with attempts = 1
    #[test]
    fn success_moves_to_published() {
        let t = next_after_attempt(0, 3, true);
        assert_eq!(
            t,
            Transition {
                status: OutboxStatus::Published,
                attempts: 1,
            }
        );
    }

    // GIVEN a pending row with retries left WHEN the publish fails
    // THEN it stays Pending with the attempt count incremented
    #[test]
    fn failure_with_retries_left_stays_pending() {
        let t = next_after_attempt(0, 3, false);
        assert_eq!(t.status, OutboxStatus::Pending);
        assert_eq!(t.attempts, 1);

        let t = next_after_attempt(1, 3, false);
        assert_eq!(t.status, OutboxStatus::Pending);
        assert_eq!(t.attempts, 2);
    }

    // GIVEN a pending row on its last allowed attempt WHEN the publish fails
    // THEN it transitions to Failed (the relay gives up)
    #[test]
    fn failure_at_cap_moves_to_failed() {
        let t = next_after_attempt(2, 3, false);
        assert_eq!(
            t,
            Transition {
                status: OutboxStatus::Failed,
                attempts: 3,
            }
        );
    }

    // GIVEN success after prior failures WHEN the publish finally succeeds
    // THEN it is Published even though earlier attempts failed
    #[test]
    fn late_success_still_publishes() {
        let t = next_after_attempt(2, 3, true);
        assert_eq!(t.status, OutboxStatus::Published);
        assert_eq!(t.attempts, 3);
    }

    // GIVEN a max_attempts of 0 (meaningless) WHEN an attempt fails
    // THEN it is clamped to 1 and the row fails after the single attempt
    #[test]
    fn zero_max_attempts_is_clamped_to_one() {
        let t = next_after_attempt(0, 0, false);
        assert_eq!(t.status, OutboxStatus::Failed);
        assert_eq!(t.attempts, 1);
    }
}
