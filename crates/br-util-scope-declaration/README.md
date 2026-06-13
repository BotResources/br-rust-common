# br-util-scope-declaration

The boot-time **scope-declaration handshake** helper. A generic BR service that
*owns scopes* declares them to Identity at startup and gates its readiness on the
confirmation, in a few lines. Thin technical wrapper (tier `util`): it
orchestrates the handshake over `br-core-integration` and `br-core-scope` and
drives `br-util-axum-readiness`; it enforces no domain policy.

## Usage — the three lines

```rust,no_run
use br_core_scope::{ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};
use br_util_axum_readiness::ReadinessHandle;
use br_util_scope_declaration::{declare_scopes, ScopeDeclarationConfig, ScopeDeclarationOutcome};

# async fn boot(
#     jetstream: async_nats::jetstream::Context,
#     readiness: ReadinessHandle, // started not_ready
# ) -> Result<(), Box<dyn std::error::Error>> {
// 1. Build the validated declaration (from br-core-scope).
let declaration = ScopeDeclaration::new(
    ServiceManifest::new(ServiceKey::new("notifier")?, "label.notifier", "desc.notifier"),
    vec![ScopeSpec::new(ScopeKey::new("notifier:read")?, "label.read", "desc.read", false)],
)?;

// 2. Declare + gate readiness (stream name + enabled flag wired from Helm).
match declare_scopes(
    &jetstream,
    declaration,
    readiness,
    ScopeDeclarationConfig::enabled("IDENTITY"), // or ::disabled(..) to opt out
)
.await?
{
    // 3. Accepted / Disabled → already UP. Rejected → already DOWN; the caller
    //    decides to stay alive out of rotation or exit.
    ScopeDeclarationOutcome::Accepted | ScopeDeclarationOutcome::Disabled => { /* serve */ }
    ScopeDeclarationOutcome::Rejected(reason) => { /* log/exit; gate is DOWN */ }
    // `ScopeDeclarationOutcome` is `#[non_exhaustive]`: match additively.
    _ => {}
}
# Ok(())
# }
```

## The handshake protocol

`declare_scopes` implements **subscribe-first / re-publish-on-timeout**:

1. Generate `correlation_id = C` **once** at startup.
2. **Subscribe first** — create the per-replica, per-boot `CorrelatedAwaiter`
   over both confirmation subjects (`identity.evt.service_scope.accepted.v1` and
   `…rejected.v1`). **Never a durable, never a queue-group**: each replica must
   see *all* confirmations and filter on its own `C`. Subscribing before
   publishing closes the race against a fast confirmation.
3. Publish the durable command `identity.cmd.service_scope.declare.v1`
   (`IntegrationCommand<DeclareServiceScopes>`, `metadata.correlation_id = C`).
4. Await the correlated confirmation. On a wait timeout → **re-publish (same
   `C`)** and keep awaiting, **indefinitely** — Identity may be down, and the
   readiness gate keeps the pod out of rotation meanwhile (an accepted
   coupling). **Duplicate confirmations are expected** (timeout re-publish +
   Identity's always-re-emit produce them); the first correlated match wins, the
   rest are ignored.
5. **Accepted** → readiness **UP**. **Rejected** → readiness **DOWN** +
   `tracing::error` with the structured reason (codes, not prose), **no retry** —
   rejection is deterministic, so re-declaring would not change it.

The awaiter stays armed across the re-publish gap up to its `inactive_threshold`
(300s by default — far above the 10s default per-wait deadline, so no extra
configuration is needed). Override the timing only for an unusual deployment.

## Disabled vs. scopeless — a deliberate distinction

| | Owns scopes? | Calls this helper? | What happens |
|---|---|---|---|
| **Enabled** | yes | yes | full handshake; readiness gated on the confirmation |
| **Disabled** | yes | yes | per-project opt-out (wired from Helm): no publish, no awaiter; readiness set **UP**; returns `Disabled` |
| **Scopeless** | **no** | **no** | nothing to declare or gate — the service does not use this helper at all |

Disabled mode sets the gate **UP** because the consumer wired the gate expecting
the helper to drive it. The scopeless case (e.g. a notification back-office that
declares no scopes and gates nothing) is a different posture entirely and never
touches this crate.

## Subjects & fail-loud infrastructure

Subjects follow the integration convention `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}`
and are fixed by the `br-scope-declaration-contract` crate (the single source of
the wire coordinates):

- command:  `identity.cmd.service_scope.declare.v1`
- accepted: `identity.evt.service_scope.accepted.v1`
- rejected: `identity.evt.service_scope.rejected.v1`

The JetStream **stream is pre-declared** (Helm / operator). The awaiter binds it
by name and **fails loud** with `IntegrationError::Consume { NoStream }` if it is
missing — this helper **never** creates a stream or a durable consumer.

Subjects are built with `br_core_integration::integration_subject` (the single
source of the subject convention) and pinned to the canonical contract strings
by a unit test.

## Declaring-service identity (provenance, not authentication)

The declare command's `metadata.actor` is a **deterministic, name-based**
service-account id derived from the service key
(`uuid_v5(crate_namespace, service_key)`, via `declaring_actor`). It identifies
*which service* authored the declaration, by convention — stable across reboots
and replicas, collision-free per key. It **authenticates nothing**: the boot bus
has no authenticated principal, and a peer with bus access could forge the same
id. The honest guarantee is "the conventional id of the named declarant", never
"this proves the sender is that service" — there is intentionally no anti-spoof
check in the contract.

## Install

```toml
[dependencies]
br-util-scope-declaration = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-scope-declaration", tag = "v0.8.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
