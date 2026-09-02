use br_core_integration::{OutboxStatus, Transition};

use crate::error::{FabricError, PublishErrorKind};
use crate::fabric::PublishOutcome;

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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RelayPass {
    pub picked: usize,
    pub published: usize,
    pub duplicates: usize,
    pub row_id_fallbacks: usize,
    pub failed: usize,
    pub retried: usize,
    pub structural: usize,
    pub min_retry_attempts: Option<u32>,
}

impl From<RelayPass> for RelayReport {
    fn from(pass: RelayPass) -> Self {
        Self {
            picked: pass.picked,
            published: pass.published,
            failed: pass.failed,
            retried: pass.retried,
            structural: pass.structural,
            min_retry_attempts: pass.min_retry_attempts,
        }
    }
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
    pass: &mut RelayPass,
    publish_result: &Result<PublishOutcome, FabricError>,
    transition: Transition,
    structural: bool,
) {
    if let Ok(outcome) = publish_result {
        pass.published += 1;
        if outcome.is_duplicate() {
            pass.duplicates += 1;
        }
        return;
    }
    if structural {
        pass.structural += 1;
        return;
    }
    if transition.status == OutboxStatus::Failed {
        pass.failed += 1;
    } else {
        pass.retried += 1;
        pass.min_retry_attempts = Some(match pass.min_retry_attempts {
            Some(prev) => prev.min(transition.attempts),
            None => transition.attempts,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored() -> Result<PublishOutcome, FabricError> {
        Ok(PublishOutcome::Stored { sequence: 1 })
    }

    fn duplicate() -> Result<PublishOutcome, FabricError> {
        Ok(PublishOutcome::Duplicate { sequence: 1 })
    }

    fn no_stream() -> Result<PublishOutcome, FabricError> {
        Err(FabricError::Publish {
            kind: PublishErrorKind::NoStream,
            detail: "no stream".into(),
        })
    }

    fn transient() -> Result<PublishOutcome, FabricError> {
        Err(FabricError::Publish {
            kind: PublishErrorKind::Timeout,
            detail: "timed out".into(),
        })
    }

    fn published(attempts: u32) -> Transition {
        Transition {
            status: OutboxStatus::Published,
            attempts,
        }
    }

    fn pending(attempts: u32) -> Transition {
        Transition {
            status: OutboxStatus::Pending,
            attempts,
        }
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
    fn pass_counts_a_success_as_published_and_not_as_a_duplicate() {
        let mut pass = RelayPass::default();
        classify_pass(&mut pass, &stored(), published(1), false);
        assert_eq!(pass.published, 1);
        assert_eq!(pass.duplicates, 0);
    }

    #[test]
    fn a_duplicate_ack_counts_as_published_and_as_a_duplicate() {
        let mut pass = RelayPass::default();
        classify_pass(&mut pass, &duplicate(), published(1), false);
        assert_eq!(
            pass.published, 1,
            "the broker accepted the row, so the relay marks it published"
        );
        assert_eq!(
            pass.duplicates, 1,
            "duplicates are a strict subset of published"
        );
    }

    #[test]
    fn pass_counts_a_transient_retry_and_tracks_attempts() {
        let mut pass = RelayPass::default();
        classify_pass(&mut pass, &transient(), pending(2), false);
        assert_eq!(pass.retried, 1);
        assert_eq!(pass.min_retry_attempts, Some(2));
    }

    #[test]
    fn pass_counts_a_structural_failure_without_burning_retry() {
        let mut pass = RelayPass::default();
        classify_pass(&mut pass, &no_stream(), pending(0), true);
        assert_eq!(pass.structural, 1);
        assert_eq!(pass.retried, 0);
        assert_eq!(pass.min_retry_attempts, None);
    }

    #[test]
    fn pass_counts_a_terminal_failure() {
        let mut pass = RelayPass::default();
        classify_pass(
            &mut pass,
            &transient(),
            Transition {
                status: OutboxStatus::Failed,
                attempts: 5,
            },
            false,
        );
        assert_eq!(pass.failed, 1);
    }

    #[test]
    fn the_report_projection_drops_only_the_counters_it_has_no_field_for() {
        let pass = RelayPass {
            picked: 7,
            published: 6,
            duplicates: 2,
            row_id_fallbacks: 3,
            failed: 1,
            retried: 4,
            structural: 5,
            min_retry_attempts: Some(2),
        };
        assert_eq!(
            RelayReport::from(pass),
            RelayReport {
                picked: 7,
                published: 6,
                failed: 1,
                retried: 4,
                structural: 5,
                min_retry_attempts: Some(2),
            }
        );
    }
}
