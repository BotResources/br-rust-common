use br_core_integration::IntegrationCommand;
use br_core_scope::{DeclareServiceScopes, ScopeDeclarationError, ServiceKey};
use br_identity_domain::{DeclarationOutcome, RejectedIdentity, judge_declaration};
use br_scope_declaration_contract::UNREPRESENTABLE_SERVICE;

use crate::conflict::SaveOutcome;
use crate::error::AppError;
use crate::publisher::ConfirmationPublisher;
use crate::repository::ScopeRegistryRepository;

const MAX_ATTEMPTS: u32 = 5;

pub struct ScopeDeclarationPipeline {
    repository: ScopeRegistryRepository,
    confirmations: ConfirmationPublisher,
}

impl ScopeDeclarationPipeline {
    pub fn new(repository: ScopeRegistryRepository, confirmations: ConfirmationPublisher) -> Self {
        Self {
            repository,
            confirmations,
        }
    }

    pub async fn handle(
        &self,
        command: &IntegrationCommand<DeclareServiceScopes>,
    ) -> Result<DeclarationOutcome, AppError> {
        for _ in 0..MAX_ATTEMPTS {
            let (mut registry, loaded_version) = self.repository.load().await?;

            match judge_declaration(&mut registry, command.payload.clone()) {
                DeclarationOutcome::Rejected { identity, reason } => {
                    return self.reject(command, identity, reason).await;
                }
                DeclarationOutcome::Accepted { service, result } => {
                    match self.repository.save(&registry, loaded_version).await? {
                        SaveOutcome::Persisted => {
                            self.confirmations
                                .publish_accepted(command, service.clone())
                                .await?;
                            return Ok(DeclarationOutcome::Accepted { service, result });
                        }
                        SaveOutcome::VersionConflict => continue,
                        SaveOutcome::ScopeConflict { scope_key, owner } => {
                            let reason = ScopeDeclarationError::ScopeOwnedByAnotherService {
                                key: scope_key,
                                owner,
                            };
                            return self
                                .reject(command, RejectedIdentity::Service(service), reason)
                                .await;
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
        identity: RejectedIdentity,
        reason: ScopeDeclarationError,
    ) -> Result<DeclarationOutcome, AppError> {
        let service = reply_service(&identity);
        self.confirmations
            .publish_rejected(command, service, reason.clone())
            .await?;
        Ok(DeclarationOutcome::Rejected { identity, reason })
    }
}

fn reply_service(identity: &RejectedIdentity) -> ServiceKey {
    match identity {
        RejectedIdentity::Service(service) => service.clone(),
        RejectedIdentity::Unrepresentable { .. } => ServiceKey::new(UNREPRESENTABLE_SERVICE)
            .expect("UNREPRESENTABLE_SERVICE is a valid service key"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_service_echoes_a_representable_identity() {
        let identity = RejectedIdentity::Service(ServiceKey::new("notifier").unwrap());
        assert_eq!(
            reply_service(&identity),
            ServiceKey::new("notifier").unwrap()
        );
    }

    #[test]
    fn reply_service_maps_an_unrepresentable_identity_to_the_named_sentinel() {
        let identity = RejectedIdentity::Unrepresentable {
            raw: "NOPE".to_string(),
        };
        assert_eq!(
            reply_service(&identity),
            ServiceKey::new(UNREPRESENTABLE_SERVICE).unwrap()
        );
    }
}
