# Changelog — br-identity-app

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.1] — 2026-06-10

**Docs**
- The `README.md` was already the crate's root doc (`#![doc = include_str!(..)]`),
  but its usage example was fenced ```` ```rust,ignore ````, so it was rendered
  but never compiled. It is now ```` ```rust,no_run ````: the example is compiled
  (and type-checked, not run) as a doctest by `cargo test`, so it can no longer
  drift from the code. The composition-root values it cannot construct purely
  (the two pools, the JetStream context, the readiness handle) are supplied
  through an `async` boot wrapper.
- Fixed the example so it compiles: the corrupt-store callback now wires a real
  `br_util_axum_readiness::ReadinessHandle::set_not_ready` (added as a dev-only
  dependency for the doctest). The previous, never-compiled example could name a
  method that did not exist — exactly the doc-must-match-code drift this change
  closes mechanically.

## [0.1.0] — 2026-06-10

**Added**
- Initial release. The Identity bounded context's **application / adapter half**
  (tier `bc`), wiring the pure `br-identity-domain` `ScopeRegistry` aggregate to
  Postgres and the NATS integration bus for the **scope-registration slice**.
  - `ScopeRegistryRepository` — the Postgres adapter. `load()` hydrates the
    aggregate from current-state rows, re-validating every invariant (the
    read-side double barrier), and returns the optimistic-lock `version`.
    `save(registry, loaded_version)` advances a one-row head-table `version`
    conditioned on the loaded value (a singleton's optimistic lock), upserts the
    services and scopes idempotently, and returns a `SaveOutcome`:
    `Persisted`, a benign `VersionConflict` (the caller retries), or a terminal
    `ScopeConflict { scope_key, owner }` — a `UNIQUE(scope_key)` violation
    **classified, not raised as an error**. The `owner` is the **actual** owning
    service, read back from the committed winner's row, so the rejection names
    the truth rather than the losing declarant; if that row has since vanished
    the conflict is downgraded to a `VersionConflict` (the caller re-judges)
    rather than fabricating an owner.
  - `migrate(pool)` — applies the embedded `scope_registry` schema **explicitly**
    (the composing service calls it on boot via its owner pool). Three tables:
    `scope_registry_head` (one row, the singleton's `version`, a `CHECK` forbids
    a second), `scope_registry_service` (manifests), and `scope_registry` (one
    row per scope; `scope_key` globally unique — the final database net behind
    the aggregate's uniqueness invariant, FK-able later by an entitlements
    table). `registered_at` / `last_seen_at` columns are **reserved for a future
    lifecycle feature** (orphan detection); no lifecycle mechanism ships now —
    `registered_at` is set once on first acceptance, `last_seen_at` is touched on
    every (re-)declaration. **No row-level security**: registry rows carry no
    per-user ownership dimension (scope-gated platform data), so RLS would add
    cost with no isolation to gain. Explicit invocation is **not**
    auto-provisioning — the schema is declared and applied by a deliberate call,
    never conjured at request time.
  - `ConfirmationPublisher` — emits the correlated `accepted` / `rejected`
    confirmations as `IntegrationEvent` envelopes on
    `identity.evt.service_scope.{accepted,rejected}.v1`. `correlation_id` echoes
    the command's; `causation_id` is the command's `command_id`; the `actor`
    echoes the command's actor (the confirmation is caused by the declarant's
    command). **Always re-emits**, including for an idempotent re-declare, so a
    rebooted replica's readiness is never stuck waiting on a swallowed reply.
  - `ScopeDeclarationPipeline` — the uniform `load → judge → save → dispatch`,
    holding **no business logic** (every verdict is the domain's
    `judge_declaration`). A version conflict retries (bounded, `MAX_ATTEMPTS = 5`,
    then `Nak(delay)`); a `UNIQUE(scope_key)` violation maps to a
    `rejected(ScopeOwnedByAnotherService)` confirmation — **never a nak** (which
    would redeliver, re-violate, and loop forever).
  - `run_scope_declarations(...)` — binds a **pre-declared** stream + durable
    consumer by name (fail-loud — the lib never auto-provisions) via
    `br-core-integration::DurableConsumer` and drives the pipeline. A
    structurally-unreadable payload is `term`ed on the consumer's poison path
    before decode (surfaced via `on_poison`); a readable-but-invalid declaration
    is judged `Rejected`, answered, and **acked** — never nak/termed. Both
    domain verdicts ack; an infrastructure fault naks. Takes an
    `on_permanent_failure: FnMut(&AppError)` callback (alongside `on_poison`),
    invoked once per delivery that fails with a permanent corrupt-store fault, so
    the composing service can drop readiness / alert. The function **returns**
    `Err` on bind failure and fatal stream termination but does not itself touch
    readiness — the **caller must observe the returned future** (and the callback)
    and wire both into its readiness gate; spawning-and-dropping loses the
    fail-loud property (documented in the rustdoc and README).
  - `AppError` — the crate's own error type (persistence, hydration, publish,
    missing head row, exhausted retries, corrupt stored key); a lower layer's
    error is never re-exposed across the public API. `is_permanent()` classifies a
    fault as a permanent corrupt-store trip (`Hydration` / `CorruptStoredKey` —
    never heals by redelivery, only by an operator PG repair) versus a transient
    fault; the consumer naks both classes but signals them differently (loud
    `error` + callback vs `warn`).
- **Outbound KV projection deliberately skipped** — Postgres remains the source
  of truth; a KV projection for low-latency scope lookups is noted as future
  work.
- **Domain events not published on the domain bus** — the slice logs the
  `CommandResult.events`; no consumer needs a domain-bus fan-out yet (the
  integration-bus confirmations are what the declarant binds to). An honest,
  documented decision, not a pretend-dispatch.
- **Tests.** Inline unit tests for the adapter's unique-violation classification,
  the `AppError::is_permanent` permanence classification (each variant), and the
  rejected-reply service-key sourcing; a full e2e suite (`#[ignore]`, gated on
  both `TEST_DATABASE_URL` and `NATS_URL`, `--test-threads=1`) against a **real**
  Postgres + NATS JetStream proving: declare → persisted rows + `accepted`
  (correlation echoed, causation = command_id); idempotent re-declare (no
  duplicate rows, version unchanged, `accepted` re-emitted); readable-but-invalid
  declaration → `rejected` with the structured reason, registry untouched, no
  redelivery loop; the `UNIQUE(scope_key)` violation classified as `ScopeConflict`
  carrying the **real** owner (the committed winner, not the losing declarant) and
  mapped to a single `rejected` confirmation with no redelivery; and the runtime
  app role's least-privilege on the registry tables.
