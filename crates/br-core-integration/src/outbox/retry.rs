use std::time::Duration;

pub const RETRY_BACKOFF_BASE: Duration = Duration::from_millis(500);

pub const RETRY_BACKOFF_MAX: Duration = Duration::from_secs(30);

pub fn retry_backoff(attempts: u32) -> Duration {
    let attempts = attempts.max(1);
    let shift = attempts - 1;
    let factor = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
    let millis = RETRY_BACKOFF_BASE
        .as_millis()
        .saturating_mul(u128::from(factor));
    let capped = millis.min(RETRY_BACKOFF_MAX.as_millis());
    Duration::from_millis(capped as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_attempt_waits_the_base() {
        assert_eq!(retry_backoff(1), RETRY_BACKOFF_BASE);
    }

    #[test]
    fn zero_attempts_is_clamped_to_the_base() {
        assert_eq!(retry_backoff(0), RETRY_BACKOFF_BASE);
    }

    #[test]
    fn backoff_doubles_per_attempt() {
        assert_eq!(retry_backoff(1), Duration::from_millis(500));
        assert_eq!(retry_backoff(2), Duration::from_millis(1000));
        assert_eq!(retry_backoff(3), Duration::from_millis(2000));
        assert_eq!(retry_backoff(4), Duration::from_millis(4000));
    }

    #[test]
    fn backoff_caps_and_never_overflows() {
        assert_eq!(retry_backoff(20), RETRY_BACKOFF_MAX);
        assert_eq!(retry_backoff(u32::MAX), RETRY_BACKOFF_MAX);
    }
}
