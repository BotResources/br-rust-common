# br-identity-domain

The Identity bounded context — **pure domain** (no I/O, no `async`, no
transport). This release covers the **scope-registration slice**: the part of
Identity that records which services exist and which `{service}:{capability}`
permission scopes each owns.

It is the `*-domain` half of a *packaged bounded context*: a real BC packaged for
silo reuse (same code, one instance per project). The companion `*-app` crate
adds persistence, the integration bus, and the `load → command → save → dispatch`
pipeline; this crate holds only the aggregate, its commands, its events, and its
invariants.

**Purpose.** A service declares its scopes to Identity; Identity's registry
either accepts the declaration (recording the service and its scopes) or rejects
it with a single structured reason. This crate is the pure decision: it judges a
declaration and produces the events, leaving all I/O to the application layer.

**When to use.** Building or testing the scope-registry logic of an Identity
service — the aggregate that enforces scope-key uniqueness and answers
"who owns this scope?".

**When not to use.** The shared *declaration language* (`ScopeKey`,
`ScopeDeclaration`, the declare/accepted/rejected payloads) is `br-core-scope`,
which both the declaring service and this crate depend on. The transport, the
persistence, and the integration replies are the application layer's, not this
crate's.

## Key distinctions

- **One aggregate, not one per service.** The invariant the registry protects —
  *a scope key is owned by at most one service* — spans all services, so it is a
  single `ScopeRegistry` aggregate. It carries a `version` for optimistic
  locking; a state-changing command bumps it once, an idempotent no-op does not.
- **State-stored, not event-sourced.** The application layer persists the
  current state as rows and uses the emitted events as deltas to dispatch — they
  are *not* the rehydration mechanism. `ScopeRegistry::hydrate` rebuilds the
  aggregate from persisted state (never by replaying a log).
- **Double barrier.** Every invariant is enforced at command time *and*
  re-validated on hydration. `hydrate` re-checks global key uniqueness, **service
  uniqueness** (a `ServiceKey` appears at most once, even with disjoint scopes),
  and scope/owner prefix consistency — a malformed persisted state fails to load
  with a `RegistryHydrationError` instead of resurrecting an illegal registry.
- **Re-declaration is label/description-immutable (v1).** Re-declaring an
  already-owned scope is a no-op for its metadata: the stored `label_key` /
  `description_key` / `platform_only` are frozen at first registration; a
  re-declare only touches `last_seen_at` (in the app layer). Changing a scope's
  copy is therefore not a registry mutation — it does not bump the version and
  emits no event.
- **Decide, don't act.** `register_declaration` mutates in-memory state and
  returns events; it performs no I/O and dispatches nothing.
- **One pure verdict function.** `judge_declaration` composes `br-core-scope`'s
  boundary validation (key syntax, prefix ownership, intra-declaration
  duplicates) with the aggregate command (cross-owner conflict + idempotency), so
  the whole accepted/rejected decision is one pure call the application layer
  makes between `load` and `save`. On rejection the registry is left untouched.
- **Domain events, not integration confirmations.** The crate emits granular,
  past-tense `RegistryEvent`s on the domain bus (internal to the BC). The
  integration-bus accepted/rejected confirmations are the application layer's
  reply, not domain events of this crate.
- **Codes, not language.** The crate's own `RegistryHydrationError` (distinct
  from `br-core-scope`'s rejection language) and every reason it forwards are
  stable codes plus structured params — human text and i18n live at the edge.

## What's inside

| Type | Role |
|---|---|
| `ScopeRegistry` | The single aggregate. `new()` (empty, version 0); `hydrate(version, services)` (rebuild from persisted state, re-validating every invariant); `register_declaration(&ScopeDeclaration)` (the command → `CommandResult`); reads `version()` / `services()` / `find_service()` / `owner_of()`. |
| `judge_declaration` | The pure receiver-side verdict: `(&mut ScopeRegistry, DeclareServiceScopes) -> DeclarationOutcome`. Boundary validation + the aggregate command in one call. |
| `DeclarationOutcome` | `Accepted { service, result }` / `Rejected { identity, reason }`. |
| `RejectedIdentity` | The identity a rejection is *about*: `Service(ServiceKey)` when the manifest key is valid, `Unrepresentable { raw }` when it is not — a typed value, never an unwrap-or-default placeholder. |
| `RegisteredService` | A service entity inside the registry: its manifest plus the scopes it owns. `key()` / `manifest()` / `scopes()`. |
| `RegistryEvent` | Granular domain events: `ServiceRegistered { service, label_key, description_key }`, `ScopeRegistered { key, owning_service, label_key, description_key, platform_only }`. Each carries its full value. |
| `CommandResult` | `{ events, warnings }`. `from_events`, `is_noop`, `Default` (the no-op result). |
| `RegistryWarning` | A typed, `#[non_exhaustive]`, empty-for-now warning enum (forward-compat). |
| `RegistryHydrationError` | Why a persisted state failed to load: `DuplicateScope`, `DuplicateService`, `ScopeOwnerMismatch`. Stable `Display` codes. |

`ScopeDeclaration` and `ScopeDeclarationError` are re-exported from
`br-core-scope` for convenience.

## Usage

```rust
use br_identity_domain::{judge_declaration, DeclarationOutcome, ScopeRegistry};
use br_core_scope::{
    DeclareServiceScopes, ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey,
    ServiceManifest,
};

// Application layer: load the registry (here: a fresh one).
let mut registry = ScopeRegistry::new();

// A service's declaration arrives off the bus as a `DeclareServiceScopes`. The
// sender built it from a validated `ScopeDeclaration`:
let manifest = ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "svc.label", "svc.desc");
let scopes = vec![ScopeSpec::new(
    ScopeKey::new("notifier:read").unwrap(),
    "scope.read.label",
    "scope.read.desc",
    false,
)];
let command = DeclareServiceScopes::new(ScopeDeclaration::new(manifest, scopes).unwrap());

// One pure call decides the whole verdict. `DeclarationOutcome` is
// `#[non_exhaustive]`, so match it with a trailing wildcard arm.
match judge_declaration(&mut registry, command) {
    DeclarationOutcome::Accepted { service, result } => {
        // Persist `registry` (its `version()` drives the optimistic-lock save),
        // dispatch `result.events`, then reply ServiceScopesAccepted { service }.
        let _ = (service, result);
    }
    DeclarationOutcome::Rejected { identity, reason } => {
        // Persist nothing; reply ServiceScopesRejected with this reason. The
        // rejected `identity` is typed: `Unrepresentable { raw }` when the
        // manifest key itself is malformed (no valid ServiceKey exists).
        let _ = (identity, reason);
    }
    _ => unreachable!("DeclarationOutcome is non_exhaustive — future variants"),
}
```

## Install

```toml
[dependencies]
br-identity-domain = { git = "https://github.com/BotResources/br-rust-common", package = "br-identity-domain", tag = "v0.11.1" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
