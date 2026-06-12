#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OutboxStatus {
    Pending,
    Published,
    Failed,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[error("unknown outbox status: {0:?}")]
pub struct UnknownOutboxStatus(pub String);

impl OutboxStatus {
    pub const fn as_db_str(self) -> &'static str {
        match self {
            Self::Pending => "PENDING",
            Self::Published => "PUBLISHED",
            Self::Failed => "FAILED",
        }
    }

    pub fn from_db_str(value: &str) -> Result<Self, UnknownOutboxStatus> {
        match value {
            "PENDING" => Ok(Self::Pending),
            "PUBLISHED" => Ok(Self::Published),
            "FAILED" => Ok(Self::Failed),
            other => Err(UnknownOutboxStatus(other.to_string())),
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Published | Self::Failed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transition {
    pub status: OutboxStatus,
    pub attempts: u32,
}

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

    #[test]
    fn unknown_db_str_is_a_typed_error() {
        let err = OutboxStatus::from_db_str("ARCHIVED").unwrap_err();
        assert_eq!(err, UnknownOutboxStatus("ARCHIVED".to_string()));
    }

    #[test]
    fn only_pending_is_non_terminal() {
        assert!(!OutboxStatus::Pending.is_terminal());
        assert!(OutboxStatus::Published.is_terminal());
        assert!(OutboxStatus::Failed.is_terminal());
    }

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

    #[test]
    fn failure_with_retries_left_stays_pending() {
        let t = next_after_attempt(0, 3, false);
        assert_eq!(t.status, OutboxStatus::Pending);
        assert_eq!(t.attempts, 1);

        let t = next_after_attempt(1, 3, false);
        assert_eq!(t.status, OutboxStatus::Pending);
        assert_eq!(t.attempts, 2);
    }

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

    #[test]
    fn late_success_still_publishes() {
        let t = next_after_attempt(2, 3, true);
        assert_eq!(t.status, OutboxStatus::Published);
        assert_eq!(t.attempts, 3);
    }

    #[test]
    fn zero_max_attempts_is_clamped_to_one() {
        let t = next_after_attempt(0, 0, false);
        assert_eq!(t.status, OutboxStatus::Failed);
        assert_eq!(t.attempts, 1);
    }
}
