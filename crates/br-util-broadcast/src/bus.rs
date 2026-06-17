use tokio::sync::broadcast::{self, Receiver};

use crate::{BroadcastError, PendingBroadcast};

#[derive(Clone, Debug)]
pub struct EventBus<T> {
    sender: broadcast::Sender<T>,
}

impl<T: Clone> EventBus<T> {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        assert!(
            capacity > 0,
            "EventBus capacity must be > 0 (a zero-capacity broadcast channel can buffer nothing); got {capacity}"
        );
        let (sender, _) = broadcast::channel(capacity);
        Self { sender }
    }

    #[must_use]
    pub fn subscribe(&self) -> Receiver<T> {
        self.sender.subscribe()
    }

    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }

    pub fn publish_after_commit(&self, pending: PendingBroadcast<T>) -> Result<(), BroadcastError> {
        let total = pending.events.len();
        for (sent, event) in pending.events.into_iter().enumerate() {
            if self.sender.send(event).is_err() {
                return Err(BroadcastError::NoSubscribers {
                    unheard: total - sent,
                });
            }
        }
        Ok(())
    }
}
