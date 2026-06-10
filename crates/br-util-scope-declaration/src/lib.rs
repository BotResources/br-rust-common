//! Boot-time **scope-declaration handshake** helper.
//!
//! A generic BR service that *owns scopes* declares them to Identity at startup
//! and gates its readiness on the confirmation, in a few lines:
//!
//! ```no_run
//! use br_core_scope::{ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};
//! use br_util_axum_readiness::ReadinessHandle;
//! use br_util_scope_declaration::{
//!     declare_scopes, ScopeDeclarationConfig, ScopeDeclarationOutcome,
//! };
//!
//! # async fn boot(
//! #     jetstream: async_nats::jetstream::Context,
//! #     readiness: ReadinessHandle,
//! # ) -> Result<(), Box<dyn std::error::Error>> {
//! // 1. Build the validated declaration (from br-core-scope).
//! let declaration = ScopeDeclaration::new(
//!     ServiceManifest::new(ServiceKey::new("notifier")?, "label.notifier", "desc.notifier"),
//!     vec![ScopeSpec::new(ScopeKey::new("notifier:read")?, "label.read", "desc.read", false)],
//! )?;
//!
//! // 2. Declare + gate readiness (stream name + enabled flag wired from Helm).
//! match declare_scopes(
//!     &jetstream,
//!     declaration,
//!     readiness,
//!     ScopeDeclarationConfig::enabled("IDENTITY"),
//! )
//! .await?
//! {
//!     // 3. Accepted / Disabled → already UP. Rejected → already DOWN; the
//!     //    caller decides to stay alive out of rotation or exit.
//!     ScopeDeclarationOutcome::Accepted | ScopeDeclarationOutcome::Disabled => { /* serve */ }
//!     ScopeDeclarationOutcome::Rejected(reason) => {
//!         tracing::error!(?reason, "scope declaration rejected; staying out of rotation");
//!     }
//!     // `ScopeDeclarationOutcome` is `#[non_exhaustive]`: match additively.
//!     _ => {}
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## The handshake protocol
//!
//! [`declare_scopes`] implements the **subscribe-first / re-publish-on-timeout**
//! protocol (full detail on the function):
//!
//! 1. Generate `correlation_id = C` **once** at startup.
//! 2. **Subscribe first** — create the per-replica, per-boot
//!    [`CorrelatedAwaiter`](br_core_integration::CorrelatedAwaiter) over both
//!    confirmation subjects (`identity.evt.service_scope.accepted.v1` and
//!    `…rejected.v1`). Never a durable, never a queue-group: each replica must
//!    see *all* confirmations and filter on its own `C`. Subscribing before
//!    publishing closes the race against a fast confirmation.
//! 3. Publish the durable command `identity.cmd.service_scope.declare.v1`
//!    (`IntegrationCommand<DeclareServiceScopes>`, `metadata.correlation_id = C`).
//! 4. Await the correlated confirmation. On a wait timeout → **re-publish (same
//!    `C`)** and keep awaiting, **indefinitely** — Identity may be down, and the
//!    readiness gate keeps the pod out of rotation meanwhile (an accepted
//!    coupling). **Duplicate confirmations are expected** (timeout re-publish +
//!    Identity's always-re-emit produce them); the first correlated match wins.
//! 5. **Accepted** → readiness **UP**. **Rejected** → readiness **DOWN** +
//!    `tracing::error` with the structured reason (codes, not prose), **no
//!    retry** — rejection is deterministic.
//!
//! ## Disabled vs. scopeless — a deliberate distinction
//!
//! - **Disabled** (`ScopeDeclarationConfig::enabled == false`): a service that
//!   *does* own scopes but whose project opted the handshake **out** (wired from
//!   Helm). The helper is still called; it skips the publish/await and sets
//!   readiness **UP** (the consumer wired the gate expecting the helper to drive
//!   it). Returns [`ScopeDeclarationOutcome::Disabled`].
//! - **Scopeless**: a service that owns **no** scopes at all. It does **not call
//!   this helper** — there is nothing to declare and nothing to gate on. (This is
//!   the `svc-notifier` posture: it declares no scopes and gates nothing.)
//!
//! ## Subject convention & fail-loud infra
//!
//! Subjects follow `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}` and are built with
//! [`integration_subject`](br_core_integration::integration_subject), never
//! formatted by hand. The JetStream **stream is pre-declared** (Helm / operator):
//! the awaiter binds it by name and **fails loud** with
//! [`IntegrationError::Consume`](br_core_integration::IntegrationError) if it is
//! missing — this helper never creates a stream or a durable.
//!
//! ## Declaring-service identity
//!
//! The declare command's `metadata.actor` is a **deterministic, name-based**
//! service id derived from the service key (see [`actor`]). It is *declarative
//! provenance* — which service authored the declaration, by convention — and
//! **authenticates nothing** (the boot bus has no authenticated principal).

pub mod actor;
mod config;
mod handshake;
mod outcome;
mod subjects;

pub use actor::declaring_actor;
pub use config::ScopeDeclarationConfig;
pub use handshake::declare_scopes;
pub use outcome::ScopeDeclarationOutcome;
