//! [`ScopeDeclarationPipeline`] — the uniform `load → judge → save → dispatch`
//! for one received `DeclareServiceScopes` command.
//!
//! This is the **orchestration** layer: it marshals I/O around the domain and
//! holds **no business logic**. Every verdict comes from
//! [`judge_declaration`](br_identity_domain::judge_declaration); this file only
//! moves bytes — load the registry, judge, persist on accept, and publish the
//! correlated confirmation either way.
//!
//! ## The loop and the two conflicts
//!
//! ```text
//!   loop (bounded):
//!     (registry, version) = repo.load()          // hydrate + double barrier
//!     outcome             = judge(registry, cmd)  // pure domain verdict
//!     match outcome:
//!       Rejected(reason)        -> publish rejected;   done
//!       Accepted(service, _):
//!         match repo.save(registry, version):
//!           Persisted               -> publish accepted;   done
//!           VersionConflict         -> retry (re-hydrate, re-judge)
//!           ScopeConflict(key,owner) -> publish rejected(ScopeOwnedByAnotherService); done
//! ```
//!
//! - A **version conflict** is a benign race → retry, up to [`MAX_ATTEMPTS`].
//!   Exhausting the cap is truly exceptional under the mono-pod write model and
//!   returns [`AppError::ConflictRetriesExhausted`] so the consumer naks with a
//!   delay for a later redelivery.
//! - A **`UNIQUE(scope_key)` violation** is the database enforcing the
//!   aggregate's invariant as the final net. It maps to a `rejected`
//!   confirmation — **never** a nak: a nak would redeliver, re-violate, and loop
//!   forever. This is the heart of the protocol: a *readable* declaration that
//!   cannot be accepted is answered, not retried.
//!
//! A structurally-unreadable payload never reaches this pipeline — the durable
//! consumer terms it on the poison path before decode. Here the payload is a
//! decoded [`DeclareServiceScopes`]; a *readable but invalid* one (a malformed
//! key, a prefix mismatch) is judged `Rejected` and answered, never nak/termed.

use br_core_integration::IntegrationCommand;
use br_core_scope::{DeclareServiceScopes, ScopeDeclarationError, ServiceKey};
use br_identity_domain::{DeclarationOutcome, judge_declaration};

use crate::conflict::SaveOutcome;
use crate::error::AppError;
use crate::publisher::ConfirmationPublisher;
use crate::repository::ScopeRegistryRepository;

/// Bounded optimistic-lock retry cap. Under the mono-pod write model writes are
/// serialized, so a version conflict is rare and a tiny cap suffices; the cap
/// exists so a future scale-out cannot spin unboundedly. Reaching it is the
/// truly-exceptional fallback (→ nak-with-delay), not the steady state.
const MAX_ATTEMPTS: u32 = 5;

/// What the pipeline did with a command. Both arms have **already published**
/// their confirmation; the consumer maps both to `Ack` (the command was
/// handled — an invalid declaration is handled by rejecting it, not by naking).
///
/// `#[non_exhaustive]`: match with a wildcard arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum HandledOutcome {
    /// The declaration was accepted (possibly an idempotent no-op) and an
    /// `accepted` confirmation was published.
    Accepted {
        /// The declaring service.
        service: ServiceKey,
    },
    /// The declaration was rejected for the given reason and a `rejected`
    /// confirmation was published.
    Rejected {
        /// Why it was refused (a stable code + params).
        reason: ScopeDeclarationError,
    },
}

/// Runs the scope-declaration pipeline. Holds the repository and the
/// confirmation publisher; injected with the shared pool + publisher at the
/// composition root.
pub struct ScopeDeclarationPipeline<P: br_core_integration::IntegrationPublisher + ?Sized> {
    repository: ScopeRegistryRepository,
    confirmations: ConfirmationPublisher<P>,
}

impl<P: br_core_integration::IntegrationPublisher + ?Sized> ScopeDeclarationPipeline<P> {
    /// Assemble the pipeline from its collaborators.
    pub fn new(
        repository: ScopeRegistryRepository,
        confirmations: ConfirmationPublisher<P>,
    ) -> Self {
        Self {
            repository,
            confirmations,
        }
    }

    /// Handle one decoded declare command end-to-end: `load → judge → save →
    /// dispatch`. On success the confirmation has been published and the
    /// [`HandledOutcome`] says which; an [`AppError`] means an infrastructure
    /// fault (or exhausted retries) the consumer should nak.
    ///
    /// # Errors
    ///
    /// [`AppError`] on a persistence/transport fault or exhausted optimistic-lock
    /// retries — never on a domain rejection (that is published as `rejected`
    /// and returned as [`HandledOutcome::Rejected`]).
    pub async fn handle(
        &self,
        command: &IntegrationCommand<DeclareServiceScopes>,
    ) -> Result<HandledOutcome, AppError> {
        for _ in 0..MAX_ATTEMPTS {
            // load — hydrate the aggregate (re-validating invariants) + version.
            let (mut registry, loaded_version) = self.repository.load().await?;

            // judge — the pure domain verdict. The command's payload is cloned
            // because judging consumes it (and a retry re-judges a fresh copy).
            match judge_declaration(&mut registry, command.payload.clone()) {
                DeclarationOutcome::Rejected { reason } => {
                    return self.reject(command, reason).await;
                }
                DeclarationOutcome::Accepted { service, .. } => {
                    // save — CAS on the loaded version.
                    match self.repository.save(&registry, loaded_version).await? {
                        SaveOutcome::Persisted => {
                            // dispatch — always emit accepted, even on a no-op.
                            self.confirmations
                                .publish_accepted(command, service.clone())
                                .await?;
                            return Ok(HandledOutcome::Accepted { service });
                        }
                        SaveOutcome::VersionConflict => continue, // benign race → retry.
                        SaveOutcome::ScopeConflict { scope_key, owner } => {
                            // The unique net fired: terminal rejection, never a
                            // nak. The repository read back the *actual* owner
                            // (the service that won the race) from the committed
                            // row, so the reason names the truth — not the losing
                            // declarant. (A vanished winner is downgraded to a
                            // VersionConflict by the repository, so reaching here
                            // means a real, settled owner.)
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

    /// Publish a `rejected` confirmation and return the matching outcome. The
    /// reply's `service` is sourced from the original payload (a rejection may
    /// itself be *about* a malformed service key), so it is read from the raw
    /// declaration rather than a validated `ServiceKey`.
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

/// The `service` to stamp on a `rejected` reply. The validated `ServiceKey` may
/// not exist (the rejection can be *about* a malformed manifest key), so source
/// it from the raw payload: use the manifest's raw service string if it is a
/// valid key, else a deterministic placeholder so the reply still carries a
/// `ServiceKey`-shaped field.
fn rejected_reply_service(command: &IntegrationCommand<DeclareServiceScopes>) -> ServiceKey {
    let raw_key = command.payload.raw().manifest.key.as_str();
    ServiceKey::new(raw_key).unwrap_or_else(|_| {
        // The manifest key itself was malformed (the very reason for rejection).
        // The reply must still carry a ServiceKey; use a reserved placeholder
        // that can never collide with a real declaring service. The structured
        // `reason` already names the offending key, so no information is lost.
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

    // The `rejected` reply's service is sourced from the raw payload: a valid
    // manifest key is echoed verbatim.
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

    // A malformed manifest key (the very thing being rejected) cannot build a
    // ServiceKey, so the reply falls back to the reserved `unknown` placeholder
    // — the structured reason still names the offending key, so nothing is lost.
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
