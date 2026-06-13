use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AwaiterConfig {
    pub inactive_threshold: Duration,
}

impl AwaiterConfig {
    pub const DEFAULT_INACTIVE_THRESHOLD: Duration = Duration::from_secs(300);
}

impl Default for AwaiterConfig {
    fn default() -> Self {
        Self {
            inactive_threshold: Self::DEFAULT_INACTIVE_THRESHOLD,
        }
    }
}
