use tokio::sync::watch;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RelayHealth {
    Healthy,
    Degraded { reason: &'static str },
}

pub const REASON_NO_STREAM: &str = "outbox.publish.no_stream";

pub type RelayHealthReceiver = watch::Receiver<RelayHealth>;

pub(crate) struct RelayHealthChannel {
    sender: watch::Sender<RelayHealth>,
    receiver: RelayHealthReceiver,
}

impl RelayHealthChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = watch::channel(RelayHealth::Healthy);
        Self { sender, receiver }
    }

    pub(crate) fn receiver(&self) -> RelayHealthReceiver {
        self.receiver.clone()
    }

    pub(crate) fn set(&self, health: RelayHealth) {
        self.sender.send_if_modified(|current| {
            if *current == health {
                false
            } else {
                *current = health;
                true
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn starts_healthy() {
        let ch = RelayHealthChannel::new();
        assert_eq!(*ch.receiver().borrow(), RelayHealth::Healthy);
    }

    #[test]
    fn publishes_a_degraded_transition() {
        let ch = RelayHealthChannel::new();
        let rx = ch.receiver();
        ch.set(RelayHealth::Degraded {
            reason: REASON_NO_STREAM,
        });
        assert_eq!(
            *rx.borrow(),
            RelayHealth::Degraded {
                reason: REASON_NO_STREAM
            }
        );
    }

    #[test]
    fn identical_state_does_not_mark_changed() {
        let ch = RelayHealthChannel::new();
        let mut rx = ch.receiver();
        rx.mark_unchanged();
        ch.set(RelayHealth::Healthy);
        assert!(!rx.has_changed().unwrap());

        ch.set(RelayHealth::Degraded {
            reason: REASON_NO_STREAM,
        });
        assert!(rx.has_changed().unwrap());
    }
}
