//! The boot-time handshake itself: subscribe-first, publish, await the
//! correlated confirmation, re-publish on timeout, drive the readiness gate.

use br_core_integration::{
    CorrelatedAwaiter, CorrelatedMatch, IntegrationCommand, IntegrationError, IntegrationEvent,
    IntegrationPublisherExt, MessageMetadata, NatsIntegrationPublisher,
};
use br_core_scope::{DeclareServiceScopes, ScopeDeclaration, ServiceKey, ServiceScopesRejected};
use br_util_axum_readiness::ReadinessHandle;
use chrono::Utc;
use uuid::Uuid;

use crate::actor::declaring_actor;
use crate::config::ScopeDeclarationConfig;
use crate::outcome::ScopeDeclarationOutcome;
use crate::subjects::{self, DeclarationSubjects};

/// Declare `declaration`'s scopes to Identity and gate `readiness` on the
/// confirmation — the few-lines boot helper.
///
/// ## Protocol (enabled mode)
///
/// 1. Generate `correlation_id = C` **once**.
/// 2. **Subscribe first**: create a per-replica, per-boot [`CorrelatedAwaiter`]
///    over both confirmation subjects (`identity.evt.service_scope.accepted.v1`
///    and `…rejected.v1`) — never a durable, never a queue-group, so this
///    replica sees all confirmations and filters its own `C`. Subscribing before
///    publishing closes the race against a fast confirmation.
/// 3. Publish the durable command `identity.cmd.service_scope.declare.v1`
///    (`IntegrationCommand<DeclareServiceScopes>`, `metadata.correlation_id = C`).
/// 4. Await the correlated confirmation. On a wait **timeout**, re-publish (same
///    `C`) and keep awaiting — **indefinitely** (Identity may be down; the
///    readiness gate keeps the pod out of rotation meanwhile, an accepted
///    coupling). Duplicate confirmations are expected and harmless: the awaiter
///    resolves on the first correlated match and ignores the rest.
/// 5. On **Accepted** → readiness **UP**, return
///    [`Accepted`](ScopeDeclarationOutcome::Accepted). On **Rejected** →
///    readiness **DOWN** + `tracing::error` with the structured reason, **no
///    retry** (rejection is deterministic), return
///    [`Rejected`](ScopeDeclarationOutcome::Rejected) for the caller to act on.
///
/// ## Disabled mode
///
/// `config.enabled == false` skips the handshake entirely — no awaiter, no
/// publish — sets readiness **UP**, and returns
/// [`Disabled`](ScopeDeclarationOutcome::Disabled). This is the per-project
/// opt-out, distinct from the intrinsic scopeless case (a service owning no
/// scopes does not call this helper at all).
///
/// ## Errors
///
/// Returns [`IntegrationError`] only for **fail-loud infrastructure** faults
/// that no retry can fix: a missing stream at awaiter-create time
/// ([`Consume { NoStream }`](IntegrationError::Consume)), the awaiter's
/// ephemeral consumer being reaped mid-protocol
/// ([`Consume { ConsumerGone }`](IntegrationError::Consume) — raise
/// `awaiter.inactive_threshold` if the gap between waits can exceed it), or a
/// transport failure while awaiting. A *publish* failure is not terminal: it is
/// logged and retried on the next loop iteration (same `C`), exactly like a
/// timeout. The readiness gate is left **DOWN** when this returns `Err`.
pub async fn declare_scopes(
    jetstream: &async_nats::jetstream::Context,
    declaration: ScopeDeclaration,
    readiness: ReadinessHandle,
    config: ScopeDeclarationConfig,
) -> Result<ScopeDeclarationOutcome, IntegrationError> {
    let service = declaration.manifest().key.clone();

    if !config.enabled {
        tracing::info!(
            service = %service,
            "scope-declaration handshake disabled (per-project opt-out); readiness UP"
        );
        readiness.set_ready();
        return Ok(ScopeDeclarationOutcome::Disabled);
    }

    let subjects = DeclarationSubjects::build();
    let correlation_id = Uuid::now_v7();

    // Subscribe FIRST — over both confirmation subjects, on the pre-declared
    // stream. Fails loud if the stream is missing; the awaiter never creates it.
    let mut awaiter = CorrelatedAwaiter::create_with(
        jetstream,
        &config.stream_name,
        subjects.confirmation_filters(),
        config.awaiter,
    )
    .await?;

    // The command envelope is built once and republished verbatim (same id, same
    // C) on every timeout — re-publishing the identical command is idempotent
    // from Identity's side and keeps the correlation stable.
    let publisher = NatsIntegrationPublisher::new(jetstream.clone());
    let command = build_command(&service, correlation_id, declaration);

    loop {
        // Publish (or re-publish). A publish failure is transient here — log and
        // fall through to await; the next iteration republishes.
        if let Err(err) = publisher.publish_command(&subjects.declare, &command).await {
            tracing::warn!(
                service = %service,
                correlation_id = %correlation_id,
                error = %err,
                "scope-declaration publish failed; will retry after the next wait"
            );
        }

        match awaiter
            .await_correlation(correlation_id, config.wait_timeout)
            .await?
        {
            // A correlated reply arrived. If it resolves to a terminal outcome,
            // return it; if it was unusable (an undecodable rejected reply — an
            // Identity-side contract break), keep awaiting rather than fabricate
            // a verdict, with the gate still DOWN.
            Some(matched) => {
                if let Some(outcome) = resolve_match(&subjects, &service, &readiness, matched) {
                    return Ok(outcome);
                }
            }
            // Timed-out wait: re-publish (same C) and keep awaiting. The awaiter
            // stays armed across this gap (its inactive_threshold is far above
            // wait_timeout), so no confirmation is missed.
            None => {
                tracing::info!(
                    service = %service,
                    correlation_id = %correlation_id,
                    "no scope-declaration confirmation yet; re-publishing and awaiting (Identity may be down — readiness stays DOWN)"
                );
            }
        }
    }
}

/// Build the durable declare command for `correlation_id`, stamping the
/// deterministic declaring-service actor (provenance, not auth — see
/// [`declaring_actor`](crate::actor::declaring_actor)).
fn build_command(
    service: &ServiceKey,
    correlation_id: Uuid,
    declaration: ScopeDeclaration,
) -> IntegrationCommand<DeclareServiceScopes> {
    let metadata = MessageMetadata::new(declaring_actor(service), correlation_id);
    IntegrationCommand::new(
        Uuid::now_v7(),
        DeclarationSubjects::command_type(),
        subjects::VERSION,
        Utc::now(),
        metadata,
        DeclareServiceScopes::new(declaration),
    )
}

/// Decode the matched confirmation and drive the readiness gate.
///
/// The subject tells us which payload to decode:
/// - `accepted` → readiness **UP**, [`Accepted`](ScopeDeclarationOutcome::Accepted);
/// - `rejected` → readiness **DOWN** + `tracing::error` (codes not prose),
///   [`Rejected`](ScopeDeclarationOutcome::Rejected).
///
/// Returns `None` only for a reply that matched our correlation on the rejected
/// subject yet **failed to decode** — an Identity-side contract break. We do not
/// fabricate a verdict from it: the caller keeps awaiting (gate still DOWN),
/// which is the honest, fail-loud-shaped response (a malformed reply is treated
/// as "not yet confirmed", and the deterministic real reply, if any, will be
/// re-emitted). An undecodable reply never becomes a false `Accepted`.
fn resolve_match(
    subjects: &DeclarationSubjects,
    service: &ServiceKey,
    readiness: &ReadinessHandle,
    matched: CorrelatedMatch,
) -> Option<ScopeDeclarationOutcome> {
    if matched.subject == subjects.accepted {
        tracing::info!(
            service = %service,
            correlation_id = %matched.metadata.correlation_id,
            "scope declaration accepted by Identity; readiness UP"
        );
        readiness.set_ready();
        return Some(ScopeDeclarationOutcome::Accepted);
    }

    // The only other filtered subject is `rejected`. Decode the structured
    // reason; an undecodable reply is a contract break — log it and return None
    // (keep awaiting) rather than invent a reason.
    let reason = match serde_json::from_slice::<IntegrationEvent<ServiceScopesRejected>>(
        &matched.payload,
    ) {
        Ok(event) => event.payload,
        Err(err) => {
            tracing::error!(
                service = %service,
                subject = %matched.subject,
                error = %err,
                "scope-declaration rejection reply failed to decode; keeping readiness DOWN and awaiting a well-formed confirmation"
            );
            return None;
        }
    };

    tracing::error!(
        service = %service,
        correlation_id = %matched.metadata.correlation_id,
        // codes-not-language: the reason's Display is a stable code.
        reason_code = %reason.reason,
        rejected_service = %reason.service,
        "scope declaration REJECTED by Identity; readiness DOWN, no retry (rejection is deterministic)"
    );
    readiness.set_not_ready(format!("scope declaration rejected: {}", reason.reason));
    Some(ScopeDeclarationOutcome::Rejected(reason))
}
