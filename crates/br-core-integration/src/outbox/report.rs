use crate::outbox::status::{OutboxStatus, Transition};

pub const DEFAULT_MAX_ATTEMPTS: u32 = 5;

pub const DEFAULT_MAX_MESSAGES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelayPolicy {
    pub max_attempts: u32,
    pub max_messages: usize,
}

impl Default for RelayPolicy {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_MAX_ATTEMPTS,
            max_messages: DEFAULT_MAX_MESSAGES,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RelayReport {
    pub picked: usize,
    pub published: usize,
    pub failed: usize,
    pub retried: usize,
    pub structural: usize,
    pub min_retry_attempts: Option<u32>,
}

pub(super) fn classify_pass(
    report: &mut RelayReport,
    publish_result: &Result<(), crate::IntegrationError>,
    transition: Transition,
    structural: bool,
) {
    use OutboxStatus::{Failed, Published};
    if publish_result.is_ok() {
        report.published += 1;
        return;
    }
    if structural {
        report.structural += 1;
        return;
    }
    if transition.status == Failed {
        report.failed += 1;
    } else {
        report.retried += 1;
        report.min_retry_attempts = Some(match report.min_retry_attempts {
            Some(prev) => prev.min(transition.attempts),
            None => transition.attempts,
        });
    }
    let _ = Published;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PublishErrorKind;

    fn no_stream() -> Result<(), crate::IntegrationError> {
        Err(crate::IntegrationError::Publish {
            kind: PublishErrorKind::NoStream,
            detail: "no stream".into(),
        })
    }

    fn transient() -> Result<(), crate::IntegrationError> {
        Err(crate::IntegrationError::Publish {
            kind: PublishErrorKind::Timeout,
            detail: "timed out".into(),
        })
    }

    #[test]
    fn default_policy_has_documented_caps() {
        let p = RelayPolicy::default();
        assert_eq!(p.max_attempts, DEFAULT_MAX_ATTEMPTS);
        assert_eq!(p.max_messages, DEFAULT_MAX_MESSAGES);
    }

    #[test]
    fn classify_counts_a_success_as_published() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Published,
            attempts: 1,
        };
        classify_pass(&mut report, &Ok(()), t, false);
        assert_eq!(report.published, 1);
        assert_eq!(report.retried, 0);
        assert_eq!(report.structural, 0);
        assert_eq!(report.min_retry_attempts, None);
    }

    #[test]
    fn classify_counts_a_transient_retry_and_tracks_attempts() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Pending,
            attempts: 2,
        };
        classify_pass(&mut report, &transient(), t, false);
        assert_eq!(report.retried, 1);
        assert_eq!(report.failed, 0);
        assert_eq!(report.structural, 0);
        assert_eq!(report.min_retry_attempts, Some(2));
    }

    #[test]
    fn min_retry_attempts_tracks_the_soonest() {
        let mut report = RelayReport::default();
        classify_pass(
            &mut report,
            &transient(),
            Transition {
                status: OutboxStatus::Pending,
                attempts: 3,
            },
            false,
        );
        classify_pass(
            &mut report,
            &transient(),
            Transition {
                status: OutboxStatus::Pending,
                attempts: 1,
            },
            false,
        );
        assert_eq!(report.min_retry_attempts, Some(1));
    }

    #[test]
    fn classify_counts_a_terminal_failure() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Failed,
            attempts: 5,
        };
        classify_pass(&mut report, &transient(), t, false);
        assert_eq!(report.failed, 1);
        assert_eq!(report.retried, 0);
        assert_eq!(report.min_retry_attempts, None);
    }

    #[test]
    fn classify_counts_a_structural_failure_without_burning_retry() {
        let mut report = RelayReport::default();
        let t = Transition {
            status: OutboxStatus::Pending,
            attempts: 0,
        };
        classify_pass(&mut report, &no_stream(), t, true);
        assert_eq!(report.structural, 1);
        assert_eq!(report.retried, 0);
        assert_eq!(report.failed, 0);
        assert_eq!(report.min_retry_attempts, None);
    }
}
