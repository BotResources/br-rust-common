use std::sync::Arc;

use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProjectorProgress {
    pub changes: u64,
}

pub type ProjectorProgressReceiver = watch::Receiver<ProjectorProgress>;

#[derive(Clone)]
pub(crate) struct ProgressChannel {
    sender: Arc<watch::Sender<ProjectorProgress>>,
}

impl ProgressChannel {
    pub(crate) fn new() -> Self {
        Self {
            sender: Arc::new(watch::Sender::new(ProjectorProgress::default())),
        }
    }

    pub(crate) fn receiver(&self) -> ProjectorProgressReceiver {
        self.sender.subscribe()
    }

    pub(crate) fn bump(&self) {
        self.sender.send_modify(|progress| progress.changes += 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_channel_starts_at_zero_changes() {
        let channel = ProgressChannel::new();
        assert_eq!(channel.receiver().borrow().changes, 0);
    }

    #[test]
    fn every_bump_is_observed_by_a_receiver_taken_before_it() {
        let channel = ProgressChannel::new();
        let receiver = channel.receiver();
        channel.bump();
        channel.bump();
        assert_eq!(receiver.borrow().changes, 2);
    }

    #[test]
    fn a_receiver_taken_after_the_bumps_reads_the_accumulated_count() {
        let channel = ProgressChannel::new();
        channel.bump();
        assert_eq!(channel.receiver().borrow().changes, 1);
    }
}
