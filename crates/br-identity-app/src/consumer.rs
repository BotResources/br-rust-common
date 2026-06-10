//! [`run_scope_declarations`] — bind the durable consumer and drive the
//! pipeline over `identity.cmd.service_scope.declare.v1`.
//!
//! This wiring binds a **pre-declared** stream + durable consumer by name
//! (fail-loud — the lib never auto-provisions) via
//! [`DurableConsumer`](br_core_integration::DurableConsumer), and runs the
//! pipeline per delivered command. Multiple replicas binding the same durable
//! name share delivery (JetStream pull work-sharing), so a command is handled
//! once across the replica set.
//!
//! ## Outcome → ack mapping (the heart of the protocol)
//!
//! | pipeline result | ack | why |
//! |---|---|---|
//! | `Ok(Accepted)` | `Ack` | handled — accepted + confirmation published |
//! | `Ok(Rejected)` | `Ack` | handled — a *readable but invalid* declaration is answered with a `rejected`, not retried (a nak would redeliver and re-reject forever) |
//! | `Err(transient)` | `Nak(delay)` | a transient infrastructure fault (DB/transport) or exhausted optimistic-lock retries — redeliver after a delay; logs at `warn` |
//! | `Err(permanent)` | `Nak(delay)` | a **corrupt-store** fault ([`AppError::is_permanent`]): a hydration-barrier trip or a corrupt stored key. Still naks (see below), but logs **loudly and distinctly** and fires the operator-remediation callback |
//!
//! A structurally-**unreadable** payload never reaches the pipeline: the durable
//! consumer's poison path `term`s it before decode (surfaced via `on_poison`),
//! so it is neither redelivered forever nor mistaken for a rejectable
//! declaration. This distinction is the heart of the protocol:
//! *unreadable → term/poison*, *readable-but-invalid → rejected*.
//!
//! ## Corrupt store vs transient fault — same ack, different signal
//!
//! A **permanent** fault ([`AppError::is_permanent`] — `Hydration` /
//! `CorruptStoredKey`) means the persisted registry is corrupt at rest: every
//! redelivery re-loads the same bad rows and re-fails identically. It is **not**
//! the declarant's fault, so it is **never** `term`ed (which would falsely tell
//! the declarant "deterministic rejection, do not retry") and **never** answered
//! `rejected`. It naks at the same [`NAK_DELAY`] cadence as a transient fault —
//! deliberately: while the store is corrupt every declarant stays NotReady, and
//! once the operator repairs the rows in PG the **redelivered** commands succeed
//! with **no restart of anything**.
//!
//! The two classes are told apart by their *signal*, not their ack:
//!
//! - **transient** → `tracing::warn!` (a DB/transport blip the next redelivery
//!   may clear);
//! - **permanent** → `tracing::error!` with the greppable `registry_store_corrupt
//!   = true` field and an operator-remediation message, **plus** the
//!   `on_permanent_failure` callback so the composing service can drop its
//!   readiness / raise an alert.
//!
//! See the crate README, "Corrupt store (operator remediation)", for the full
//! posture.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use br_core_integration::{
    Delivery, DurableConsumer, IntegrationCommand, IntegrationError, IntegrationPublisher,
    MessageOutcome,
};
use br_core_scope::DeclareServiceScopes;

use crate::error::AppError;
use crate::pipeline::{HandledOutcome, ScopeDeclarationPipeline};

/// The redelivery delay applied when a command naks on a transient fault. Short
/// enough to recover promptly, long enough not to hot-loop against a momentarily
/// unavailable dependency.
const NAK_DELAY: Duration = Duration::from_secs(5);

/// Bind the pre-declared durable consumer on `stream_name` / `consumer_name`
/// and run the pipeline until the message stream ends or a fatal transport
/// error occurs. Parks at zero CPU between deliveries (never polls).
///
/// `on_poison` receives a structurally-unreadable payload's
/// [`IntegrationError::Decode`] (already `term`ed by the consumer); a service
/// typically logs it and increments a metric.
///
/// `on_permanent_failure` is invoked **once per delivery** that fails with a
/// permanent, corrupt-store fault ([`AppError::is_permanent`] — a hydration
/// barrier trip or a corrupt stored key). It exists so the **composing service
/// can drop its readiness / raise an alert** when the persisted registry is
/// corrupt at rest. Processing **continues** after it returns — the delivery is
/// nak'd (recovery is redelivery after the operator repairs PG, not a restart),
/// so the callback may fire repeatedly while the store stays corrupt; it must be
/// idempotent (e.g. flip a readiness flag, increment a gauge). A service that
/// does not need the signal passes a no-op closure (`|_| {}`).
///
/// # The caller contract — observe the returned future or lose fail-loud
///
/// This function **only returns** `Err`; it does **not** itself touch readiness.
/// It returns `Err` on two fatal conditions:
///
/// - **bind failure** — a missing pre-declared stream/consumer is a fail-loud
///   `NoStream` / `NoConsumer` ([`AppError::Publish`] wrapping the
///   [`IntegrationError`]); the lib never creates them;
/// - **fatal stream termination** — the bound consumer vanished server-side or a
///   non-recoverable transport error ended the message stream.
///
/// The composing service **MUST observe the returned future and wire it (and
/// `on_permanent_failure`) into its readiness gate** — e.g. select on it
/// alongside the server, and mark readiness DOWN if it resolves. **Spawning this
/// future and dropping the handle silently loses the fail-loud property**: the
/// consumer dies, the `Err` is discarded, and the service serves on with a dead
/// declaration path. (No readiness wrapper ships here — wiring is the composing
/// service's composition-root concern.)
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
    // Share the callback into every per-delivery handler future. The handler is
    // `FnMut(..) -> Future` and the returned future is awaited *after* the closure
    // returns, so it cannot borrow a captured `&mut G`; a shared, cheaply-cloned
    // handle is what the future can own. The lock is held only for the brief,
    // non-async callback invocation (never across an `.await`).
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

/// Run one decoded command through the pipeline and map its result to the
/// JetStream ack. Both domain verdicts ack (the command was handled); a failure
/// naks for redelivery — a permanent corrupt-store fault and a transient one
/// alike, told apart by the signal (loud error + callback vs `warn`), not the
/// ack. `on_permanent_failure` fires once per permanent-failure delivery.
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
        Ok(HandledOutcome::Accepted { service }) => {
            tracing::info!(%service, "scope declaration accepted");
            MessageOutcome::Ack
        }
        Ok(HandledOutcome::Rejected { reason }) => {
            // A readable-but-invalid declaration: answered with a `rejected`
            // confirmation, then acked. Never nak/term — the declarant has its
            // verdict and a redelivery would only re-reject.
            tracing::info!(reason = %reason, "scope declaration rejected");
            MessageOutcome::Ack
        }
        Err(err) if err.is_permanent() => {
            // A permanent, corrupt-store fault: the persisted registry is corrupt
            // at rest, so every redelivery re-fails identically until an operator
            // repairs the rows in PG. We still nak (NOT term, NOT reject): the
            // command is not the declarant's fault, declarants must stay NotReady
            // while the store is corrupt, and the redelivered commands succeed
            // after the manual fix with no restart. Signal it loudly and
            // distinctly, and fire the readiness/alert callback (idempotent — it
            // may fire on every redelivery while corruption persists).
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
            // A transient infrastructure fault (or exhausted optimistic-lock
            // retries): nak with a delay for a later redelivery. NOT a term —
            // the command is still valid and may succeed once the dependency
            // recovers.
            tracing::warn!(error = %err, "scope declaration handling failed; will retry");
            MessageOutcome::Nak(Some(NAK_DELAY))
        }
    }
}
