use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum MessageOutcome {
    Ack,
    Nak(Option<Duration>),
    Term,
}
