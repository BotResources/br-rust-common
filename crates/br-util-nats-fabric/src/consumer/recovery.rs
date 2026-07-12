use crate::consumer::backoff::Backoff;
use crate::consumer::source::MessageSource;
use crate::error::{ConsumeErrorKind, FabricError};

pub(crate) const OTHER_FAILURE_BUDGET: u32 = 10;

pub(crate) struct Recovery {
    backoff: Backoff,
    other_failures: u32,
}

impl Recovery {
    pub(crate) fn new(backoff: Backoff) -> Self {
        Self {
            backoff,
            other_failures: 0,
        }
    }

    pub(crate) fn note_delivery(&mut self) {
        self.backoff.reset();
        self.other_failures = 0;
    }

    pub(crate) async fn recover<S: MessageSource>(
        &mut self,
        source: &mut S,
        trigger: FabricError,
    ) -> Result<(), FabricError> {
        let mut current = trigger;
        let mut attempt: u32 = 0;
        loop {
            if counts_against_budget(&current) {
                self.other_failures += 1;
                if self.other_failures > OTHER_FAILURE_BUDGET {
                    return Err(current);
                }
            }
            attempt += 1;
            let delay = self.backoff.sleep().await;
            tracing::warn!(
                consume_kind = ?consume_kind(&current),
                attempt,
                delay_ms = delay.as_millis() as u64,
                other_failures = self.other_failures,
                "re-binding durable consumer after a recoverable stream error"
            );
            match source.rebind().await {
                Ok(()) => return Ok(()),
                Err(err) if should_recover(&err) => current = err,
                Err(err) => return Err(err),
            }
        }
    }
}

pub(crate) fn should_recover(error: &FabricError) -> bool {
    matches!(
        error,
        FabricError::Consume {
            kind: ConsumeErrorKind::HeartbeatMissed | ConsumeErrorKind::Other,
            ..
        }
    )
}

fn counts_against_budget(error: &FabricError) -> bool {
    matches!(
        error,
        FabricError::Consume {
            kind: ConsumeErrorKind::Other,
            ..
        }
    )
}

fn consume_kind(error: &FabricError) -> Option<ConsumeErrorKind> {
    match error {
        FabricError::Consume { kind, .. } => Some(*kind),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn consume(kind: ConsumeErrorKind) -> FabricError {
        FabricError::consume(kind, "test")
    }

    #[test]
    fn heartbeat_and_other_are_recoverable() {
        assert!(should_recover(&consume(ConsumeErrorKind::HeartbeatMissed)));
        assert!(should_recover(&consume(ConsumeErrorKind::Other)));
    }

    #[test]
    fn confirmed_absent_kinds_are_terminal() {
        assert!(!should_recover(&consume(ConsumeErrorKind::NoStream)));
        assert!(!should_recover(&consume(ConsumeErrorKind::NoConsumer)));
        assert!(!should_recover(&consume(ConsumeErrorKind::ConsumerGone)));
    }

    #[test]
    fn only_the_other_class_is_budgeted() {
        assert!(counts_against_budget(&consume(ConsumeErrorKind::Other)));
        assert!(!counts_against_budget(&consume(
            ConsumeErrorKind::HeartbeatMissed
        )));
    }
}
