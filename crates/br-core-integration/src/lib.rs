//! Typed envelopes and a publisher trait for cross-bounded-context
//! (integration) messaging.
//!
//! Where [`br-core-events`] holds the shapes that travel *inside* a bounded
//! context's event store, this crate holds the shapes that travel *between*
//! contexts on the message bus.
//!
//! ## Types
//!
//! - [`MessageMetadata`] — actor / correlation / causation. This is
//!   `br_core_events::EventMetadata` re-exported under the integration name:
//!   one type, one wire contract, no hand-synced duplicate. It carries the
//!   typed `br_core_kernel::Actor` and the backward-compatible wire format
//!   (flat `actor_id` + optional `actor_kind`; a legacy payload defaults to a
//!   human actor).
//! - [`IntegrationEvent<T>`] — fact published by an emitting context.
//! - [`IntegrationCommand<T>`] — request asking a receiving context to act.
//!
//! All envelopes are `#[non_exhaustive]`; construct them through their `new`
//! constructors.
//!
//! ## Publishing
//!
//! [`IntegrationPublisher`] is **object-safe** so applications can hold an
//! `Arc<dyn IntegrationPublisher>`. The [`IntegrationPublisherExt`] blanket
//! provides typed helpers ([`publish_event`](IntegrationPublisherExt::publish_event),
//! [`publish_command`](IntegrationPublisherExt::publish_command), and their
//! `_if_connected` fire-and-forget counterparts). A failed publish surfaces as
//! [`IntegrationError::Publish`] carrying a classified [`PublishErrorKind`].
//!
//! ## Implementations bundled here
//!
//! - [`NatsIntegrationPublisher`] — JetStream publisher; awaits the delivery
//!   ack on [`publish`](IntegrationPublisher::publish); logs and swallows errors
//!   on [`publish_if_connected`](IntegrationPublisher::publish_if_connected).
//! - [`NoopIntegrationPublisher`] — for tests.
//!
//! ## Subject naming convention
//!
//! Subjects follow `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}`
//! (e.g. `identity.evt.user.created.v1`, `notifier.cmd.notification.send.v1`).
//! Build them with [`integration_subject`] rather than formatting strings by
//! hand — the helper validates the segments and is the single source of the
//! convention. Subscribers use NATS wildcards (`identity.evt.>`,
//! `notifier.cmd.>`) to consume relevant streams.
//!
//! [`br-core-events`]: https://github.com/BotResources/br-rust-common/tree/main/crates/br-core-events

mod envelopes;
mod error;
mod nats;
mod noop;
mod publisher;
mod subject;

// Re-exported so consumers can construct `MessageMetadata` (whose `new` takes
// an `Actor`) from this crate alone, without adding a kernel dependency.
pub use br_core_events::{Actor, ServiceAccountId, UserId};
pub use envelopes::{IntegrationCommand, IntegrationEvent, MessageMetadata};
pub use error::{IntegrationError, PublishErrorKind};
pub use nats::NatsIntegrationPublisher;
pub use noop::NoopIntegrationPublisher;
pub use publisher::{IntegrationPublisher, IntegrationPublisherExt};
pub use subject::{MessageKind, SubjectError, integration_subject};
