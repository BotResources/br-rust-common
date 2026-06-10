# Changelog — br-identity-domain

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-06-10

**Added**
- Initial release. The Identity bounded context's **pure domain** (no I/O, no
  `async`, no transport) — the **scope-registration slice** only. Depends on
  `br-core-scope` for the shared declaration language; the user/PAT kernel and
  the rest of Identity are out of scope.
  - `ScopeRegistry` — the **single aggregate** holding every registered service
    and its scopes. The invariant it owns is *a scope key is owned by at most one
    service*, which spans all services, so it is one aggregate (not one per
    service). Carries a monotonic `version` for optimistic locking
    (`version()`), incremented once per state-changing command and **not** bumped
    by an idempotent no-op. Read surface (`services()`, `find_service()`,
    `owner_of()`) is the same aggregate as the write path — no separate read
    model.
  - `ScopeRegistry::register_declaration(&ScopeDeclaration) -> Result<CommandResult,
    ScopeDeclarationError>` — the command. Takes a *validated* `ScopeDeclaration`
    (syntax + prefix-ownership + intra-declaration duplicates already proven by
    `br-core-scope`) and judges the registry invariants: cross-owner conflict
    (`ScopeOwnedByAnotherService`, atomic — nothing is partially registered) and
    idempotent re-declaration (a key the service already owns emits no event).
    Decides only; performs no I/O.
  - `ScopeRegistry::new()` / `ScopeRegistry::hydrate(version, services)` — the
    **double barrier**. `hydrate` rebuilds the aggregate from persisted *state*
    (state-stored, not event-sourced — never log replay) and **re-validates every
    cross-row invariant** (global key uniqueness, scope/owner prefix consistency);
    a malformed persisted state fails to load with a `RegistryHydrationError`.
  - `judge_declaration(&mut ScopeRegistry, DeclareServiceScopes) ->
    DeclarationOutcome` — the **pure receiver-side decision function**. Composes
    `br-core-scope`'s boundary validation (`InvalidScopeKey`,
    `ScopePrefixMismatch`, `DuplicateScopeInDeclaration`) with the aggregate
    command (`ScopeOwnedByAnotherService` + idempotency), so the whole
    accepted/rejected verdict is one pure call. On rejection the registry is left
    untouched.
  - `DeclarationOutcome` — `Accepted { service, result }` /
    `Rejected { reason }`. `#[non_exhaustive]`.
  - `RegisteredService` — the per-service child entity (manifest + owned scopes),
    so a grant-admin read surface can group scopes by service. Private scope
    collection, mutated only through the registry root.
  - `RegistryEvent` — the granular, past-tense **domain events** (domain bus,
    unprefixed, internal to the BC): `ServiceRegistered { service, label_key,
    description_key }` and `ScopeRegistered { key, owning_service, label_key,
    description_key, platform_only }`. Each carries its full value so a subscriber
    never re-queries. No coarse `Updated`. `#[non_exhaustive]`.
  - `CommandResult { events, warnings }` — what a command returns;
    `from_events`, `is_noop`, `Default` (the no-op result). `warnings` is a
    `Vec<RegistryWarning>` (a typed, `#[non_exhaustive]`, empty-for-now enum) —
    carried for forward-compat; no warning case exists in this slice.
  - `RegistryHydrationError` — **this crate's own** error type (distinct from the
    shared `ScopeDeclarationError` rejection language): `DuplicateScope`,
    `ScopeOwnerMismatch`. Per codes-not-language, its `Display` strings are stable
    codes. `#[non_exhaustive]`.
- **Scope of "what this crate does not do":**
  - It emits **domain events** only. The integration-bus accepted/rejected
    confirmations (`ServiceScopesAccepted` / `ServiceScopesRejected`) are the
    application layer's reply, not domain events of this crate.
  - It does **not** convert `RegistryEvent` to the generic
    `br-core-events::RawEvent`/`DomainEvent` envelope: the registry is a singleton
    with no natural per-aggregate id, and the envelope id / metadata /
    `aggregate_id` are supplied at persistence time, so that lowering belongs to
    the application layer. This keeps the domain dependency-minimal
    (`br-core-scope` only) and pure.
