use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct WatchProgress {
    pub changes: u64,
    pub skipped: u64,
}

impl WatchProgress {
    pub fn observed(&self) -> u64 {
        self.changes.saturating_add(self.skipped)
    }
}

pub type WatchProgressReceiver = watch::Receiver<WatchProgress>;

pub(crate) struct WatchProgressChannel {
    sender: watch::Sender<WatchProgress>,
    receiver: WatchProgressReceiver,
}

impl WatchProgressChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = watch::channel(WatchProgress::default());
        Self { sender, receiver }
    }

    pub(crate) fn receiver(&self) -> WatchProgressReceiver {
        self.receiver.clone()
    }

    pub(crate) fn snapshot(&self) -> WatchProgress {
        *self.receiver.borrow()
    }

    pub(crate) fn record_change(&self) {
        self.sender
            .send_modify(|p| p.changes = p.changes.saturating_add(1));
    }

    pub(crate) fn record_skip(&self) {
        self.sender
            .send_modify(|p| p.skipped = p.skipped.saturating_add(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_at_zero_on_both_counters() {
        let ch = WatchProgressChannel::new();
        assert_eq!(ch.snapshot(), WatchProgress::default());
        assert_eq!(ch.snapshot().observed(), 0);
    }

    #[test]
    fn a_delivered_change_and_a_skipped_entry_both_count_as_observed() {
        let ch = WatchProgressChannel::new();
        let rx = ch.receiver();
        ch.record_change();
        ch.record_skip();
        ch.record_skip();
        let progress = *rx.borrow();
        assert_eq!(progress.changes, 1);
        assert_eq!(progress.skipped, 2);
        assert_eq!(progress.observed(), 3);
    }
}
