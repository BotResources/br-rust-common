use br_core_integration::{CommandCoords, EventMetadata, IntegrationCommand, IntegrationEvent};
use br_core_scope::{
    DeclareServiceScopes, DeclareServiceScopesV2, ScopeDeclaration, ScopeDeclarationV2, ServiceKey,
    ServiceScopesRejected,
};
use br_scope_declaration_contract::{VERSION, VERSION_V2};
use br_util_axum_readiness::ReadinessHandle;
use br_util_nats_fabric::{CorrelatedMatch, Fabric, FabricError};
use chrono::Utc;
use serde::Serialize;
use uuid::Uuid;

use crate::actor::declaring_actor;
use crate::config::ScopeDeclarationConfig;
use crate::outcome::ScopeDeclarationOutcome;
use crate::subjects::DeclarationCoords;

pub async fn declare_scopes(
    fabric: &Fabric,
    declaration: ScopeDeclaration,
    readiness: ReadinessHandle,
    config: ScopeDeclarationConfig,
) -> Result<ScopeDeclarationOutcome, FabricError> {
    let service = declaration.manifest().key.clone();
    let coords = DeclarationCoords::build();
    let declare = coords.declare.clone();
    run_handshake(
        fabric,
        &service,
        coords,
        &declare,
        readiness,
        config,
        |correlation_id| build_command(&service, correlation_id, declaration),
    )
    .await
}

pub async fn declare_scopes_v2(
    fabric: &Fabric,
    declaration: ScopeDeclarationV2,
    readiness: ReadinessHandle,
    config: ScopeDeclarationConfig,
) -> Result<ScopeDeclarationOutcome, FabricError> {
    let service = declaration.manifest().key.clone();
    let coords = DeclarationCoords::build();
    let declare = coords.declare_v2.clone();
    run_handshake(
        fabric,
        &service,
        coords,
        &declare,
        readiness,
        config,
        |correlation_id| build_command_v2(&service, correlation_id, declaration),
    )
    .await
}

async fn run_handshake<T: Serialize>(
    fabric: &Fabric,
    service: &ServiceKey,
    coords: DeclarationCoords,
    declare: &CommandCoords,
    readiness: ReadinessHandle,
    config: ScopeDeclarationConfig,
    command: impl FnOnce(Uuid) -> IntegrationCommand<T>,
) -> Result<ScopeDeclarationOutcome, FabricError> {
    if !config.enabled {
        tracing::info!(
            service = %service,
            "scope-declaration handshake disabled (per-project opt-out); readiness UP"
        );
        readiness.set_ready();
        return Ok(ScopeDeclarationOutcome::Disabled);
    }

    let correlation_id = Uuid::now_v7();

    let mut awaiter = fabric.await_events(&coords.confirmation_coords()).await?;

    let command = command(correlation_id);

    loop {
        if let Err(err) = fabric.publish_command(declare, &command).await {
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
                if let Some(outcome) = resolve_match(&coords, service, &readiness, matched) {
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
    let metadata = EventMetadata::new(declaring_actor(service), correlation_id);
    IntegrationCommand::new(
        Uuid::now_v7(),
        DeclarationCoords::command_type(),
        VERSION,
        Utc::now(),
        metadata,
        DeclareServiceScopes::new(declaration),
    )
}

fn build_command_v2(
    service: &ServiceKey,
    correlation_id: Uuid,
    declaration: ScopeDeclarationV2,
) -> IntegrationCommand<DeclareServiceScopesV2> {
    let metadata = EventMetadata::new(declaring_actor(service), correlation_id);
    IntegrationCommand::new(
        Uuid::now_v7(),
        DeclarationCoords::command_type(),
        VERSION_V2,
        Utc::now(),
        metadata,
        DeclareServiceScopesV2::new(declaration),
    )
}

fn resolve_match(
    coords: &DeclarationCoords,
    service: &ServiceKey,
    readiness: &ReadinessHandle,
    matched: CorrelatedMatch,
) -> Option<ScopeDeclarationOutcome> {
    if matched.subject == coords.accepted_subject {
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

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_scope::{ScopeKey, ScopeSpec, ScopeSpecV2, ServiceManifest};
    use br_util_nats_fabric::command_subject;

    fn manifest() -> ServiceManifest {
        ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "l", "d")
    }

    fn declaration_v2() -> ScopeDeclarationV2 {
        ScopeDeclarationV2::new(
            manifest(),
            vec![
                ScopeSpecV2::new(ScopeKey::new("notifier:read").unwrap(), "l", "d", false),
                ScopeSpecV2::new(ScopeKey::new("notifier:admin").unwrap(), "l", "d", false)
                    .with_implies([ScopeKey::new("notifier:read").unwrap()])
                    .with_service_accounts_only(true),
            ],
        )
        .unwrap()
    }

    #[test]
    fn v2_command_is_a_version_2_envelope_around_the_v2_payload() {
        let service = ServiceKey::new("notifier").unwrap();
        let correlation_id = Uuid::now_v7();
        let command = build_command_v2(&service, correlation_id, declaration_v2());
        let json = serde_json::to_value(&command).unwrap();

        assert_eq!(json["command_type"], "service_scope.declare");
        assert_eq!(json["version"], 2);
        assert_eq!(
            json["metadata"]["correlation_id"],
            correlation_id.to_string()
        );
        let admin = &json["payload"]["declaration"]["scopes"][1];
        assert_eq!(admin["key"], "notifier:admin");
        assert_eq!(admin["implies"], serde_json::json!(["notifier:read"]));
        assert_eq!(admin["service_accounts_only"], true);

        let payload: DeclareServiceScopesV2 =
            serde_json::from_value(json["payload"].clone()).unwrap();
        assert_eq!(payload.validate().unwrap(), declaration_v2());
    }

    #[test]
    fn v1_command_is_unchanged() {
        let service = ServiceKey::new("notifier").unwrap();
        let declaration = ScopeDeclaration::new(
            manifest(),
            vec![ScopeSpec::new(
                ScopeKey::new("notifier:read").unwrap(),
                "l",
                "d",
                false,
            )],
        )
        .unwrap();
        let json =
            serde_json::to_value(build_command(&service, Uuid::now_v7(), declaration)).unwrap();

        assert_eq!(json["version"], 1);
        let scope = &json["payload"]["declaration"]["scopes"][0];
        assert!(scope.get("implies").is_none());
        assert!(scope.get("service_accounts_only").is_none());
    }

    #[test]
    fn v2_and_v1_publish_on_their_own_subjects() {
        let coords = DeclarationCoords::build();
        assert_eq!(
            command_subject(&coords.declare),
            "integration.cmd.identity.service_scope.declare.v1"
        );
        assert_eq!(
            command_subject(&coords.declare_v2),
            "integration.cmd.identity.service_scope.declare.v2"
        );
    }
}
