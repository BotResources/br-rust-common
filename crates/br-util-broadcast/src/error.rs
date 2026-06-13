use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum BroadcastError {
    #[error("no_subscribers unheard={unheard}")]
    NoSubscribers { unheard: usize },
}
