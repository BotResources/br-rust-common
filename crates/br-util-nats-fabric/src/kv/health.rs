use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WatchHealth {
    Healthy,
    Degraded,
}

pub type WatchHealthReceiver = watch::Receiver<WatchHealth>;

pub(crate) struct WatchHealthChannel {
    sender: watch::Sender<WatchHealth>,
    receiver: WatchHealthReceiver,
}

impl WatchHealthChannel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = watch::channel(WatchHealth::Healthy);
        Self { sender, receiver }
    }

    pub(crate) fn receiver(&self) -> WatchHealthReceiver {
        self.receiver.clone()
    }

    pub(crate) fn set(&self, health: WatchHealth) {
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
        let ch = WatchHealthChannel::new();
        assert_eq!(*ch.receiver().borrow(), WatchHealth::Healthy);
    }

    #[test]
    fn publishes_a_degraded_transition() {
        let ch = WatchHealthChannel::new();
        let rx = ch.receiver();
        ch.set(WatchHealth::Degraded);
        assert_eq!(*rx.borrow(), WatchHealth::Degraded);
    }
}
