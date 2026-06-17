use br_core_integration::{OutboxStatus, Transition};

use crate::error::{FabricError, PublishErrorKind};

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureClass {
    Structural,
    Transient,
}

pub fn classify_failure(err: &FabricError) -> FailureClass {
    match err {
        FabricError::Publish {
            kind: PublishErrorKind::NoStream,
            ..
        } => FailureClass::Structural,
        _ => FailureClass::Transient,
    }
}

pub(super) fn classify_pass(
    report: &mut RelayReport,
    publish_result: &Result<(), FabricError>,
    transition: Transition,
    structural: bool,
) {
    if publish_result.is_ok() {
        report.published += 1;
        return;
    }
    if structural {
        report.structural += 1;
        return;
    }
    if transition.status == OutboxStatus::Failed {
        report.failed += 1;
    } else {
        report.retried += 1;
        report.min_retry_attempts = Some(match report.min_retry_attempts {
            Some(prev) => prev.min(transition.attempts),
            None => transition.attempts,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_stream() -> Result<(), FabricError> {
        Err(FabricError::Publish {
            kind: PublishErrorKind::NoStream,
            detail: "no stream".into(),
        })
    }

    fn transient() -> Result<(), FabricError> {
        Err(FabricError::Publish {
            kind: PublishErrorKind::Timeout,
            detail: "timed out".into(),
        })
    }

    #[test]
    fn no_stream_is_structural() {
        assert_eq!(
            classify_failure(&no_stream().unwrap_err()),
            FailureClass::Structural
        );
    }

    #[test]
    fn timeout_is_transient() {
        assert_eq!(
            classify_failure(&transient().unwrap_err()),
            FailureClass::Transient
        );
    }

    #[test]
    fn pass_counts_a_success_as_published() {
        let mut report = RelayReport::default();
        classify_pass(
            &mut report,
            &Ok(()),
            Transition {
                status: OutboxStatus::Published,
                attempts: 1,
            },
            false,
        );
        assert_eq!(report.published, 1);
    }

    #[test]
    fn pass_counts_a_transient_retry_and_tracks_attempts() {
        let mut report = RelayReport::default();
        classify_pass(
            &mut report,
            &transient(),
            Transition {
                status: OutboxStatus::Pending,
                attempts: 2,
            },
            false,
        );
        assert_eq!(report.retried, 1);
        assert_eq!(report.min_retry_attempts, Some(2));
    }

    #[test]
    fn pass_counts_a_structural_failure_without_burning_retry() {
        let mut report = RelayReport::default();
        classify_pass(
            &mut report,
            &no_stream(),
            Transition {
                status: OutboxStatus::Pending,
                attempts: 0,
            },
            true,
        );
        assert_eq!(report.structural, 1);
        assert_eq!(report.retried, 0);
        assert_eq!(report.min_retry_attempts, None);
    }

    #[test]
    fn pass_counts_a_terminal_failure() {
        let mut report = RelayReport::default();
        classify_pass(
            &mut report,
            &transient(),
            Transition {
                status: OutboxStatus::Failed,
                attempts: 5,
            },
            false,
        );
        assert_eq!(report.failed, 1);
    }
}
