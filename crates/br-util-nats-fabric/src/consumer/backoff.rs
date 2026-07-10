use std::time::Duration;

const INITIAL: Duration = Duration::from_millis(200);
const CAP: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy)]
pub(crate) struct Backoff {
    current: Duration,
    initial: Duration,
    cap: Duration,
}

impl Backoff {
    pub(crate) fn production() -> Self {
        Self::new(INITIAL, CAP)
    }

    pub(crate) fn new(initial: Duration, cap: Duration) -> Self {
        Self {
            current: initial,
            initial,
            cap,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.current = self.initial;
    }

    pub(crate) fn next_delay(&mut self) -> Duration {
        let delay = self.current;
        self.current = self.current.saturating_mul(2).min(self.cap);
        delay
    }

    pub(crate) async fn sleep(&mut self) -> Duration {
        let delay = self.next_delay();
        tokio::time::sleep(delay).await;
        delay
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doubles_each_step_until_the_cap() {
        let mut backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(1));
        assert_eq!(backoff.next_delay(), Duration::from_millis(100));
        assert_eq!(backoff.next_delay(), Duration::from_millis(200));
        assert_eq!(backoff.next_delay(), Duration::from_millis(400));
        assert_eq!(backoff.next_delay(), Duration::from_millis(800));
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
        assert_eq!(backoff.next_delay(), Duration::from_secs(1));
    }

    #[test]
    fn reset_returns_to_the_initial_delay() {
        let mut backoff = Backoff::new(Duration::from_millis(100), Duration::from_secs(1));
        backoff.next_delay();
        backoff.next_delay();
        backoff.reset();
        assert_eq!(backoff.next_delay(), Duration::from_millis(100));
    }

    #[test]
    fn a_zero_schedule_stays_zero_for_fast_tests() {
        let mut backoff = Backoff::new(Duration::ZERO, Duration::ZERO);
        assert_eq!(backoff.next_delay(), Duration::ZERO);
        assert_eq!(backoff.next_delay(), Duration::ZERO);
    }

    #[test]
    fn production_starts_sub_second_and_caps_at_thirty_seconds() {
        let mut backoff = Backoff::production();
        assert_eq!(backoff.next_delay(), Duration::from_millis(200));
        for _ in 0..12 {
            backoff.next_delay();
        }
        assert_eq!(backoff.next_delay(), Duration::from_secs(30));
    }
}
