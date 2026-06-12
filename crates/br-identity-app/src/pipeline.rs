use br_core_integration::IntegrationCommand;
use br_core_scope::{DeclareServiceScopes, ScopeDeclarationError, ServiceKey};
use br_identity_domain::{DeclarationOutcome, judge_declaration};

use crate::conflict::SaveOutcome;
use crate::error::AppError;
use crate::publisher::ConfirmationPublisher;
use crate::repository::ScopeRegistryRepository;

const MAX_ATTEMPTS: u32 = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandledOutcome {
    Accepted {
        service: ServiceKey,
    },
    Rejected {
        reason: ScopeDeclarationError,
    },
}

pub struct ScopeDeclarationPipeline<P: br_core_integration::IntegrationPublisher + ?Sized> {
    repository: ScopeRegistryRepository,
    confirmations: ConfirmationPublisher<P>,
}

impl<P: br_core_integration::IntegrationPublisher + ?Sized> ScopeDeclarationPipeline<P> {
    pub fn new(
        repository: ScopeRegistryRepository,
        confirmations: ConfirmationPublisher<P>,
    ) -> Self {
        Self {
            repository,
            confirmations,
        }
    }

    pub async fn handle(
        &self,
        command: &IntegrationCommand<DeclareServiceScopes>,
    ) -> Result<HandledOutcome, AppError> {
        for _ in 0..MAX_ATTEMPTS {
            let (mut registry, loaded_version) = self.repository.load().await?;

            match judge_declaration(&mut registry, command.payload.clone()) {
                DeclarationOutcome::Rejected { reason } => {
                    return self.reject(command, reason).await;
                }
                DeclarationOutcome::Accepted { service, .. } => {
                    match self.repository.save(&registry, loaded_version).await? {
                        SaveOutcome::Persisted => {
                            self.confirmations
                                .publish_accepted(command, service.clone())
                                .await?;
                            return Ok(HandledOutcome::Accepted { service });
                        }
                        SaveOutcome::VersionConflict => continue,
                        SaveOutcome::ScopeConflict { scope_key, owner } => {
                            let reason = ScopeDeclarationError::ScopeOwnedByAnotherService {
                                key: scope_key,
                                owner,
                            };
                            return self.reject(command, reason).await;
                        }
                    }
                }
                _ => unreachable!("DeclarationOutcome is non_exhaustive but fully matched above"),
            }
        }
        Err(AppError::ConflictRetriesExhausted {
            attempts: MAX_ATTEMPTS,
        })
    }

    async fn reject(
        &self,
        command: &IntegrationCommand<DeclareServiceScopes>,
        reason: ScopeDeclarationError,
    ) -> Result<HandledOutcome, AppError> {
        let service = rejected_reply_service(command);
        self.confirmations
            .publish_rejected(command, service, reason.clone())
            .await?;
        Ok(HandledOutcome::Rejected { reason })
    }
}

fn rejected_reply_service(command: &IntegrationCommand<DeclareServiceScopes>) -> ServiceKey {
    let raw_key = command.payload.raw().manifest.key.as_str();
    ServiceKey::new(raw_key).unwrap_or_else(|_| {
        ServiceKey::new("unknown").expect("static placeholder service key is valid")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use br_core_integration::{Actor, MessageMetadata, UserId};
    use chrono::Utc;
    use uuid::Uuid;

    fn command(json: &str) -> IntegrationCommand<DeclareServiceScopes> {
        let metadata =
            MessageMetadata::new(Actor::Human(UserId::from(Uuid::now_v7())), Uuid::now_v7());
        IntegrationCommand::new(
            Uuid::now_v7(),
            "service_scope.declare",
            1,
            Utc::now(),
            metadata,
            serde_json::from_str(json).expect("valid declare payload json"),
        )
    }

    #[test]
    fn rejected_reply_echoes_a_valid_manifest_key() {
        let cmd = command(
            r#"{"declaration":{"manifest":{"key":"notifier","label_key":"l","description_key":"d"},"scopes":[]}}"#,
        );
        assert_eq!(
            rejected_reply_service(&cmd),
            ServiceKey::new("notifier").unwrap()
        );
    }

    #[test]
    fn rejected_reply_falls_back_when_manifest_key_is_malformed() {
        let cmd = command(
            r#"{"declaration":{"manifest":{"key":"NOPE","label_key":"l","description_key":"d"},"scopes":[]}}"#,
        );
        assert_eq!(
            rejected_reply_service(&cmd),
            ServiceKey::new("unknown").unwrap()
        );
    }
}
