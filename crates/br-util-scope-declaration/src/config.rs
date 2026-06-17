use std::time::Duration;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ScopeDeclarationConfig {
    pub enabled: bool,
    pub wait_timeout: Duration,
}

impl ScopeDeclarationConfig {
    pub const DEFAULT_WAIT_TIMEOUT: Duration = Duration::from_secs(10);

    pub fn enabled() -> Self {
        Self {
            enabled: true,
            wait_timeout: Self::DEFAULT_WAIT_TIMEOUT,
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Self::enabled()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_uses_default_timings() {
        let c = ScopeDeclarationConfig::enabled();
        assert!(c.enabled);
        assert_eq!(c.wait_timeout, ScopeDeclarationConfig::DEFAULT_WAIT_TIMEOUT);
    }

    #[test]
    fn disabled_flips_only_the_flag() {
        let c = ScopeDeclarationConfig::disabled();
        assert!(!c.enabled);
    }
}
