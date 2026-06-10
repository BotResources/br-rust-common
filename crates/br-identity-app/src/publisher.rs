//! [`ConfirmationPublisher`] — emits the `accepted` / `rejected` confirmations
//! of the scope-declaration handshake onto the integration bus.
//!
//! Identity replies to a `identity.cmd.service_scope.declare.v1` command with an
//! [`IntegrationEvent`] on:
//!
//! - `identity.evt.service_scope.accepted.v1` —
//!   [`ServiceScopesAccepted`](br_core_scope::ServiceScopesAccepted);
//! - `identity.evt.service_scope.rejected.v1` —
//!   [`ServiceScopesRejected`](br_core_scope::ServiceScopesRejected).
//!
//! ## Correlation, causation, actor
//!
//! - **`correlation_id`** echoes the command's `metadata.correlation_id` — the
//!   declarant (and its `CorrelatedAwaiter`) correlates the reply to its
//!   command on this value.
//! - **`causation_id`** is the command's `command_id` — the confirmation is the
//!   direct effect of that command.
//! - **`actor`** echoes the command's actor. The confirmation is *caused by the
//!   declarant's command*, so attributing it to the same actor keeps the causal
//!   chain honest and is what a downstream audit expects (the alternative — a
//!   synthetic Identity service actor — would invent an actor the command did
//!   not carry; we don't, there is no separate Identity machine identity in this
//!   slice to attribute it to).
//!
//! ## Always re-emit
//!
//! The publisher **always** emits, including for an idempotent re-declare (which
//! the domain judges `Accepted` with an empty result): a rebooted replica
//! re-declaring its scopes must receive its `accepted` so its readiness is never
//! stuck waiting on a confirmation that a "nothing changed, skip the reply"
//! optimization would have swallowed.

use br_core_integration::{
    IntegrationCommand, IntegrationEvent, IntegrationPublisher, IntegrationPublisherExt,
    MessageKind, MessageMetadata, integration_subject,
};
use br_core_scope::{
    ScopeDeclarationError, ServiceKey, ServiceScopesAccepted, ServiceScopesRejected,
};
use chrono::Utc;
use uuid::Uuid;

use crate::error::AppError;

/// The bounded-context segment of every confirmation subject.
const BC: &str = "identity";
/// The aggregate segment (snake_case, per the subject convention).
const AGGREGATE: &str = "service_scope";
/// The published-contract version of the confirmation payloads.
const VERSION: u8 = 1;

/// Publishes the accepted/rejected confirmations for the scope-registration
/// slice. Holds the shared integration publisher.
pub struct ConfirmationPublisher<P: IntegrationPublisher + ?Sized> {
    publisher: std::sync::Arc<P>,
}

impl<P: IntegrationPublisher + ?Sized> ConfirmationPublisher<P> {
    /// Bind the publisher.
    pub fn new(publisher: std::sync::Arc<P>) -> Self {
        Self { publisher }
    }

    /// Publish `ServiceScopesAccepted { service }` on
    /// `identity.evt.service_scope.accepted.v1`, correlated to `command` and
    /// caused by it. Always called on an `Accepted` verdict — including the
    /// idempotent no-op.
    pub async fn publish_accepted<T>(
        &self,
        command: &IntegrationCommand<T>,
        service: ServiceKey,
    ) -> Result<(), AppError> {
        let payload = ServiceScopesAccepted::new(service);
        self.publish("accepted", command, payload).await
    }

    /// Publish `ServiceScopesRejected { service, reason }` on
    /// `identity.evt.service_scope.rejected.v1`, correlated to `command` and
    /// caused by it.
    pub async fn publish_rejected<T>(
        &self,
        command: &IntegrationCommand<T>,
        service: ServiceKey,
        reason: ScopeDeclarationError,
    ) -> Result<(), AppError> {
        let payload = ServiceScopesRejected::new(service, reason);
        self.publish("rejected", command, payload).await
    }

    /// Build the correlated `IntegrationEvent` envelope and publish it on the
    /// `{name}` confirmation subject, awaiting the broker ack (a lost
    /// confirmation must surface — the declarant's readiness depends on it).
    async fn publish<T, Pay: serde::Serialize + Send + Sync>(
        &self,
        name: &str,
        command: &IntegrationCommand<T>,
        payload: Pay,
    ) -> Result<(), AppError> {
        let subject = integration_subject(BC, MessageKind::Evt, AGGREGATE, name, VERSION)
            .expect("static confirmation subject segments are valid");

        // Echo the command's actor + correlation; cause = the command's id.
        let metadata =
            MessageMetadata::new(command.metadata.actor, command.metadata.correlation_id)
                .with_causation(command.command_id);

        let event = IntegrationEvent::new(
            Uuid::now_v7(),
            format!("{AGGREGATE}.{name}"),
            VERSION,
            Utc::now(),
            metadata,
            payload,
        );

        self.publisher.publish_event(&subject, &event).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_integration::{Actor, NoopIntegrationPublisher, UserId};
    use br_core_scope::DeclareServiceScopes;
    use std::sync::Arc;

    fn command() -> IntegrationCommand<DeclareServiceScopes> {
        // A declare command with a known correlation + actor to assert the echo.
        let correlation = Uuid::now_v7();
        let metadata =
            MessageMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), correlation);
        IntegrationCommand::new(
            Uuid::now_v7(),
            "service_scope.declare",
            1,
            Utc::now(),
            metadata,
            // The payload is not inspected by the publisher; build a minimal one.
            serde_json::from_str(
                r#"{"declaration":{"manifest":{"key":"notifier","label_key":"l","description_key":"d"},"scopes":[]}}"#,
            )
            .unwrap(),
        )
    }

    // The confirmation subjects are the documented contract; the builder must
    // produce them exactly.
    #[test]
    fn confirmation_subjects_match_the_contract() {
        assert_eq!(
            integration_subject(BC, MessageKind::Evt, AGGREGATE, "accepted", VERSION).unwrap(),
            "identity.evt.service_scope.accepted.v1"
        );
        assert_eq!(
            integration_subject(BC, MessageKind::Evt, AGGREGATE, "rejected", VERSION).unwrap(),
            "identity.evt.service_scope.rejected.v1"
        );
    }

    // Publishing through the noop publisher exercises the envelope-building path
    // (correlation echo, causation = command_id, actor echo) without a broker.
    #[tokio::test]
    async fn publish_accepted_builds_a_correlated_envelope() {
        let publisher = Arc::new(NoopIntegrationPublisher);
        let confirmations = ConfirmationPublisher::new(publisher);
        let cmd = command();
        // Does not error against the noop publisher; the envelope is built and
        // the typed helper accepts it.
        confirmations
            .publish_accepted(&cmd, ServiceKey::new("notifier").unwrap())
            .await
            .expect("noop publish_accepted");
    }
}
