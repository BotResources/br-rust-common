use std::time::Duration;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ScopeDeclarationConfig {
    pub enabled: bool,
    pub stream_name: String,
    pub wait_timeout: Duration,
}

impl ScopeDeclarationConfig {
    pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn enabled(stream_name: impl Into<String>) -> Self {
        Self {
            enabled: true,
            stream_name: stream_name.into(),
            wait_timeout: Self::DEFAULT_WAIT_TIMEOUT,
        }
    }

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
    }

    #[test]
    fn disabled_flips_only_the_flag() {
        let c = ScopeDeclarationConfig::disabled("IDENTITY");
        assert!(!c.enabled);
        assert_eq!(c.stream_name, "IDENTITY");
    }
}
