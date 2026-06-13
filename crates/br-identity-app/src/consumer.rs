use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_core_integration::{
    Delivery, DurableConsumer, IntegrationCommand, IntegrationError, IntegrationPublisher,
    MessageOutcome,
};
use br_core_scope::DeclareServiceScopes;

use br_identity_domain::DeclarationOutcome;

use crate::error::AppError;
use crate::pipeline::ScopeDeclarationPipeline;

const NAK_DELAY: Duration = Duration::from_secs(5);

pub async fn run_scope_declarations<P, F, G>(
    jetstream: &async_nats::jetstream::Context,
    stream_name: &str,
    consumer_name: &str,
    pipeline: Arc<ScopeDeclarationPipeline<P>>,
    on_poison: F,
    on_permanent_failure: G,
) -> Result<(), AppError>
where
    P: IntegrationPublisher + ?Sized + Send + Sync + 'static,
    F: FnMut(IntegrationError) + Send,
    G: FnMut(&AppError) + Send + 'static,
{
    let on_permanent_failure = Arc::new(Mutex::new(on_permanent_failure));
    let consumer = DurableConsumer::bind(jetstream, stream_name, consumer_name).await?;
    consumer
        .run_commands(
            move |delivery: Delivery<IntegrationCommand<DeclareServiceScopes>>| {
                let pipeline = pipeline.clone();
                let on_permanent_failure = on_permanent_failure.clone();
                async move {
                    handle_delivery(&pipeline, delivery.envelope, on_permanent_failure).await
                }
            },
            on_poison,
        )
        .await?;
    Ok(())
}

async fn handle_delivery<P, G>(
    pipeline: &ScopeDeclarationPipeline<P>,
    command: IntegrationCommand<DeclareServiceScopes>,
    on_permanent_failure: Arc<Mutex<G>>,
) -> MessageOutcome
where
    P: IntegrationPublisher + ?Sized,
    G: FnMut(&AppError) + Send,
{
    match pipeline.handle(&command).await {
        Ok(DeclarationOutcome::Accepted { service, .. }) => {
            tracing::info!(%service, "scope declaration accepted");
            MessageOutcome::Ack
        }
        Ok(DeclarationOutcome::Rejected { reason }) => {
            tracing::info!(reason = %reason, "scope declaration rejected");
            MessageOutcome::Ack
        }
        Ok(_) => MessageOutcome::Ack,
        Err(err) if err.is_permanent() => {
            tracing::error!(
                error = %err,
                registry_store_corrupt = true,
                "scope registry store is CORRUPT at rest (hydration barrier / corrupt stored \
                 key); declarations are nak'd and stay NotReady — OPERATOR must repair the \
                 scope_registry rows in Postgres, after which redeliveries succeed (no restart)"
            );
            if let Ok(mut cb) = on_permanent_failure.lock() {
                cb(&err);
            }
            MessageOutcome::Nak(Some(NAK_DELAY))
        }
        Err(err) => {
            tracing::warn!(error = %err, "scope declaration handling failed; will retry");
            MessageOutcome::Nak(Some(NAK_DELAY))
        }
    }
}
