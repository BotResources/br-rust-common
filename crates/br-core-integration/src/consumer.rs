//! The **receiver shape**: a durable JetStream pull-consumer wrapper that
//! parks at zero CPU on `consumer.messages()` and never polls in a loop.
//!
//! This is the shape a context uses to *consume* commands or events addressed
//! to it — e.g. Identity consuming `identity.cmd.service_scope.declare.v1`. It
//! binds a **pre-declared** durable consumer by name on a **pre-declared**
//! stream (the lib never auto-provisions — a missing stream or consumer is a
//! fail-loud [`IntegrationError::Consume`] with kind `NoStream` / `NoConsumer`)
//! and runs a typed handler over the delivered messages.
//!
//! ## Delivery semantics (honest, no exactly-once)
//!
//! JetStream is **at-least-once**: a message is redelivered until it is
//! explicitly acked or termed. The handler returns a [`MessageOutcome`] per
//! message and the wrapper applies the matching ack:
//!
//! - `Ack` → handled, not redelivered.
//! - `Nak(delay)` → retry later (the explicit redelivery path).
//! - `Term` → never redeliver (an unprocessable message the handler rejects).
//!
//! A handler that wants effective once-only processing must itself be
//! **idempotent** (de-dupe on the envelope id); the transport does not provide
//! it.
//!
//! ## Queue-group semantics
//!
//! Multiple workers (e.g. a replica set) that each [`bind`](DurableConsumer::bind)
//! the **same durable consumer name** on the same stream **share** its delivery:
//! JetStream load-balances undelivered messages across the bound pull workers,
//! so each message is handled by one worker. This is the JetStream pull-consumer
//! work-sharing model — it is *not* a core-NATS queue group (no `queue`
//! subscribe), and it is honest to call it shared durable delivery, not a queue
//! group.
//!
//! ## Poison messages
//!
//! A delivered payload that does not deserialize into the expected typed
//! envelope is a **poison message**. It never reaches the handler with garbage:
//! the wrapper `term`s it (so it is not redelivered forever) and surfaces it
//! through the `on_poison` hook with an [`IntegrationError::Decode`]. This is
//! fail-closed — never a silent drop, never an infinite redelivery loop.
//!
//! ## No graceful drain (API limitation, 0.3.0)
//!
//! `run_commands` / `run_events` own the message stream until it ends or a fatal
//! transport error occurs; there is **no graceful-shutdown / drain** hook. To
//! stop a consumer you abort its task (e.g. drop the `JoinHandle` / `abort()`).
//! A message **in flight at the moment of abort** is neither acked nor naked, so
//! JetStream redelivers it after the consumer's `AckWait` — at-least-once
//! delivery already covers correctness, but expect some **redelivery latency on
//! rollouts** (a message being handled when a replica is killed is re-handled by
//! another after `AckWait`). A cooperative drain (e.g. a `CancellationToken`
//! variant that stops pulling new messages and finishes the in-flight one before
//! returning) is a planned **additive** addition — not in 0.3.0.

use std::future::Future;

use async_nats::jetstream::consumer::PullConsumer;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

use crate::nats_classify::{classify_consumer_info, classify_get_stream, classify_messages_error};
use crate::{IntegrationCommand, IntegrationError, IntegrationEvent, MessageOutcome};

/// A message delivered to a durable consumer, decoded into a typed envelope and
/// carrying the JetStream message handle so the handler can inspect delivery
/// metadata if needed.
///
/// The handler receives the typed `payload` (an [`IntegrationCommand<T>`] or
/// [`IntegrationEvent<T>`]) and returns a [`MessageOutcome`]; it never touches
/// the raw bytes or the ack wire.
#[non_exhaustive]
pub struct Delivery<E> {
    /// The subject the message arrived on.
    pub subject: String,
    /// The decoded typed envelope.
    pub envelope: E,
}

/// A durable JetStream pull-consumer wrapper bound to a pre-declared stream and
/// durable consumer. See the [module docs](crate::consumer) for the delivery,
/// queue-group, and poison-message semantics.
pub struct DurableConsumer {
    consumer: PullConsumer,
    stream_name: String,
    consumer_name: String,
}

impl DurableConsumer {
    /// Bind to a pre-declared durable consumer by name on a pre-declared
    /// stream. Fails loud — the lib never provisions JetStream objects:
    ///
    /// - the stream is missing → [`IntegrationError::Consume`] with
    ///   [`ConsumeErrorKind::NoStream`](crate::ConsumeErrorKind::NoStream);
    /// - the durable consumer is missing →
    ///   [`ConsumeErrorKind::NoConsumer`](crate::ConsumeErrorKind::NoConsumer).
    pub async fn bind(
        jetstream: &async_nats::jetstream::Context,
        stream_name: impl Into<String>,
        consumer_name: impl Into<String>,
    ) -> Result<Self, IntegrationError> {
        let stream_name = stream_name.into();
        let consumer_name = consumer_name.into();

        let stream = jetstream
            .get_stream(&stream_name)
            .await
            .map_err(|e| IntegrationError::consume(classify_get_stream(&e), e.to_string()))?;
        let consumer: PullConsumer = stream.get_consumer(&consumer_name).await.map_err(|e| {
            // `get_consumer` boxes its error; downcast to the typed
            // `ConsumerInfoError` to classify a missing consumer/stream, else
            // fall back to `Other` with the boxed message.
            match e.downcast_ref::<async_nats::jetstream::context::ConsumerInfoError>() {
                Some(info_err) => IntegrationError::consume(
                    classify_consumer_info(info_err),
                    info_err.to_string(),
                ),
                None => IntegrationError::consume(crate::ConsumeErrorKind::Other, e.to_string()),
            }
        })?;

        Ok(Self {
            consumer,
            stream_name,
            consumer_name,
        })
    }

    /// The stream this consumer is bound to.
    pub fn stream_name(&self) -> &str {
        &self.stream_name
    }

    /// The durable consumer name this wrapper is bound to.
    pub fn consumer_name(&self) -> &str {
        &self.consumer_name
    }

    /// Run the consumer over delivered [`IntegrationCommand<T>`] messages until
    /// the stream ends or a fatal transport error occurs. Parks at zero CPU
    /// between deliveries (`consumer.messages()` — never a `fetch()` loop).
    ///
    /// `handler` is called per successfully decoded command and returns the
    /// [`MessageOutcome`] to apply. `on_poison` is called for an undeserializable
    /// payload (which is `term`ed automatically — see the module docs).
    ///
    /// There is no graceful drain: stopping means aborting the task, and a
    /// message in flight at abort is redelivered after `AckWait` (see the
    /// [module docs](crate::consumer) "No graceful drain").
    pub async fn run_commands<T, H, HFut, P>(
        self,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), IntegrationError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationCommand<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(IntegrationError),
    {
        self.run_inner::<IntegrationCommand<T>, _, _, _>(&mut handler, &mut on_poison)
            .await
    }

    /// Run the consumer over delivered [`IntegrationEvent<T>`] messages. See
    /// [`run_commands`](Self::run_commands) for the contract.
    pub async fn run_events<T, H, HFut, P>(
        self,
        mut handler: H,
        mut on_poison: P,
    ) -> Result<(), IntegrationError>
    where
        T: DeserializeOwned,
        H: FnMut(Delivery<IntegrationEvent<T>>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(IntegrationError),
    {
        self.run_inner::<IntegrationEvent<T>, _, _, _>(&mut handler, &mut on_poison)
            .await
    }

    async fn run_inner<E, H, HFut, P>(
        self,
        handler: &mut H,
        on_poison: &mut P,
    ) -> Result<(), IntegrationError>
    where
        E: DeserializeOwned,
        H: FnMut(Delivery<E>) -> HFut,
        HFut: Future<Output = MessageOutcome>,
        P: FnMut(IntegrationError),
    {
        let mut messages = self.consumer.messages().await.map_err(|e| {
            IntegrationError::consume(crate::ConsumeErrorKind::Other, e.to_string())
        })?;

        while let Some(message) = messages.next().await {
            // A stream-pull failure mid-run: a `ConsumerDeleted` /
            // `MissingHeartbeat` means the bound consumer vanished server-side →
            // classify as `ConsumerGone` (fail loud); preserve the source text.
            let message = message.map_err(|e| {
                IntegrationError::consume(classify_messages_error(&e), e.to_string())
            })?;
            let subject = message.subject.as_str().to_string();

            match serde_json::from_slice::<E>(&message.payload) {
                Ok(envelope) => {
                    let outcome = handler(Delivery { subject, envelope }).await;
                    apply_outcome(&message, outcome).await;
                }
                Err(decode_err) => {
                    // Poison message: term so it is not redelivered forever,
                    // then surface it. Never a silent drop.
                    let _ = message.ack_with(async_nats::jetstream::AckKind::Term).await;
                    on_poison(IntegrationError::decode(subject, &decode_err));
                }
            }
        }

        Ok(())
    }
}

/// Apply the handler's outcome to the JetStream message. An ack-wire failure is
/// logged, not propagated: the message will simply be redelivered (at-least-once
/// already covers a lost ack), and failing the whole consumer on a transient
/// ack error would be worse than a redelivery.
async fn apply_outcome(message: &async_nats::jetstream::Message, outcome: MessageOutcome) {
    if let Err(err) = message.ack_with(outcome.into()).await {
        tracing::warn!(
            error = %err,
            subject = %message.subject,
            ?outcome,
            "failed to send ack for handled message; it may be redelivered"
        );
    }
}
