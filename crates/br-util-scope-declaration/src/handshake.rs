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

    let mut awaiter = CorrelatedAwaiter::create_with(
        jetstream,
        &config.stream_name,
        subjects.confirmation_filters(),
        config.awaiter,
    )
    .await?;

    let publisher = NatsIntegrationPublisher::new(jetstream.clone());
    let command = build_command(&service, correlation_id, declaration);

    loop {
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
            Some(matched) => {
                if let Some(outcome) = resolve_match(&subjects, &service, &readiness, matched) {
                    return Ok(outcome);
                }
            }
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
        reason_code = %reason.reason,
        rejected_service = %reason.service,
        "scope declaration REJECTED by Identity; readiness DOWN, no retry (rejection is deterministic)"
    );
    readiness.set_not_ready(format!("scope declaration rejected: {}", reason.reason));
    Some(ScopeDeclarationOutcome::Rejected(reason))
}
