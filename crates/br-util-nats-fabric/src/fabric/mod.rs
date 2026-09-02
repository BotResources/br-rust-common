mod connection;
mod outcome;
mod publish;

pub use connection::{ConnectionState, NatsAuth};
pub use outcome::PublishOutcome;

#[derive(Clone)]
pub struct Fabric {
    jetstream: async_nats::jetstream::Context,
}

impl Fabric {
    pub fn new(jetstream: async_nats::jetstream::Context) -> Self {
        Self { jetstream }
    }

    pub(crate) fn context(&self) -> &async_nats::jetstream::Context {
        &self.jetstream
    }
}
