# br-identity-app

The Identity bounded context — **application / adapter half**. This release
covers the **scope-registration slice**: it wires the pure
[`br-identity-domain`](../br-identity-domain) `ScopeRegistry` aggregate to
Postgres persistence and the NATS integration bus, and runs the uniform
`load → judge → save → dispatch` pipeline that turns a received
*declare-service-scopes* command into a persisted registry and a correlated
accepted/rejected confirmation.

It is the `*-app` half of a *packaged bounded context*: a real BC packaged for
silo reuse (same code, one instance per project). The `*-domain` crate holds the
aggregate, commands, events and invariants; **this crate holds only
orchestration and adapters — no business logic.** Every verdict comes from the
domain's `judge_declaration`; this crate moves bytes.

## What it does

A declaring service publishes an `IntegrationCommand<DeclareServiceScopes>` on
`identity.cmd.service_scope.declare.v1`. This crate:

1. **consumes** it with a durable NATS consumer (work-shared across replicas);
2. **loads** the registry from Postgres, re-validating every invariant on
   hydration (the read-side double barrier);
3. **judges** the declaration with the pure domain function;
4. **saves** the new state under an optimistic lock (on accept);
5. **dispatches** a correlated `accepted` / `rejected` confirmation on
   `identity.evt.service_scope.{accepted,rejected}.v1`.

## Persistence (Postgres is the source of truth)

Three tables, shipped as migrations by this crate (see `migrations/`):

| Table | Role |
|---|---|
| `scope_registry_head` | one row, holds the singleton aggregate's optimistic-lock `version` (a `CHECK` forbids a second row) |
| `scope_registry_service` | one row per registered service (its manifest) |
| `scope_registry` | one row per owned scope; `scope_key` is **globally unique** (the PK) — the final database net behind the aggregate's uniqueness invariant; an `org_entitlements.scope` column can FK onto it later (that table is out of scope here) |

`scope_registry` also carries `registered_at` / `last_seen_at`, **reserved for a
future lifecycle feature** (orphan detection). No lifecycle mechanism ships now; the
columns are free today and spare a later migration. Semantics implemented:
`registered_at` is set once on the scope's first acceptance and never changed;
`last_seen_at` is touched (set to `now()`) on every (re-)declaration that
references the scope, including an idempotent re-declare of an already-owned
scope.

**Versioning a singleton.** The aggregate is a singleton with one `version`, so
the optimistic lock is **one global version on the one-row head table**, not a
per-row version: the invariant spans every service, so a single token guards the
whole aggregate. The save path conditions its head `UPDATE … WHERE version =
<loaded>`; a zero-row update is a concurrency conflict.

**No RLS.** Registry rows carry no per-user ownership dimension — the registry is
scope-gated platform data (every authenticated caller reads the same registry),
not per-tenant rows. Row-level security would add policy-maintenance cost with no
isolation to gain, so it is deliberately absent (the correct engineering call per
the platform's RLS doctrine). The runtime app role gets only
`SELECT/INSERT/UPDATE` on the three tables (`br-util-postgres::grant_app_access`).

**Explicit migration, never auto-provisioning.** `migrate(pool)` applies the
embedded migrations only when the composing service calls it on boot, through its
owner/migration pool. The schema objects are *declared* and applied by a
deliberate operator call — never created on-demand at request time. A service
that never migrates runs against the schema as it finds it and fails loud, rather
than conjuring tables.

## The two conflicts (settled decisions)

- **Optimistic-lock (version) conflict** → **retry** the handler: re-hydrate,
  re-judge, re-save, up to a small bounded cap (`MAX_ATTEMPTS = 5`). Reaching the
  cap is truly exceptional under the mono-pod write model and is the fallback to
  a `Nak(delay)` for a later redelivery.
- **`UNIQUE(scope_key)` violation** → **`rejected(ScopeOwnedByAnotherService)`**,
  **never a nak**. A nak would redeliver, re-violate, and loop forever. The
  unique index is the final net behind the aggregate invariant; when it fires the
  declaration is answered with a terminal rejection.

## Readable vs unreadable (the heart of the protocol)

- A **structurally-unreadable** payload (not a valid `IntegrationCommand`
  envelope) never reaches the pipeline: the durable consumer's poison path
  `term`s it before decode and surfaces it through `on_poison`. Never a silent
  drop, never an infinite redelivery loop.
- A **readable-but-invalid** declaration (a malformed key, a prefix mismatch, a
  duplicate, or a cross-owner conflict) is judged `Rejected` and answered with a
  correlated `rejected` confirmation, then **acked**. Never nak/term — the
  declarant has its verdict and a redelivery would only re-reject.
- A **transient infrastructure fault** (a DB/transport blip, or exhausted
  optimistic-lock retries) is `Nak(delay)`ed for redelivery and logged at `warn`.
- A **permanent corrupt-store fault** (the persisted registry trips the
  hydration barrier, or holds a key the validated types reject) is **also**
  `Nak(delay)`ed — but logged loudly and distinctly, and the `on_permanent_failure`
  callback fires. See "Corrupt store (operator remediation)" below.

## Readiness contract (`run_scope_declarations`)

`run_scope_declarations` **only returns** `Err`; it does **not** itself touch the
service's readiness. It returns `Err` on **bind failure** (a missing pre-declared
stream/consumer — a fail-loud `NoStream` / `NoConsumer`, since the lib never
creates them) and on **fatal stream termination** (the bound consumer vanished
server-side, or a non-recoverable transport error ended the message stream).

The **composing service MUST observe the returned future and wire it — and the
`on_permanent_failure` callback — into its readiness gate.** Concretely: select
on the future alongside the HTTP server and mark readiness DOWN if it resolves,
and have `on_permanent_failure` drop readiness / raise an alert. **Spawning this
future and dropping the handle silently loses the fail-loud property**: the
consumer dies, the `Err` is discarded, and the service keeps serving with a dead
declaration path. No readiness wrapper ships here — wiring is the composing
service's composition-root concern.

## Corrupt store (operator remediation)

The scope registry is **lifecycle-less** at rest: there is no automatic repair, no
"quarantine and skip" path. If a persisted state ever becomes corrupt, the
deliberate platform posture is to **stop accepting declarations, signal loudly,
and wait for an operator** — not to paper over it.

**What triggers it.** The read-side **double barrier** trips on load: either the
domain's **hydration barrier** rejects a cross-row inconsistency (a duplicate
scope, a scope whose prefix disagrees with its recorded owner, a scope filed
under a service with no manifest row), or a stored key fails re-validation
(**corrupt stored key** — a value the validated `ScopeKey` / `ServiceKey` types
reject). Either is `AppError::is_permanent()`.

**What the consumer does.** It treats the failure as **permanent** — a redelivery
re-loads the same corrupt rows and re-fails identically, so it cannot heal on its
own. It deliberately does **not** `term` the command (that would falsely tell the
declarant "deterministic rejection, do not retry") and does **not** reply
`rejected` (the command is valid; the fault is the store's). Instead it:

1. logs at **`error`** with the greppable field `registry_store_corrupt = true`
   and a message naming the operator remediation (once per delivery);
2. fires the `on_permanent_failure` callback so the composing service drops its
   readiness / raises an alert;
3. **`Nak`s at the `NAK_DELAY` (5s) cadence** — the same as a transient fault.

While the store stays corrupt, every declarant's handshake therefore stays
**NotReady** (its `accepted` never arrives), which is the correct system
semantics: a registry that cannot be trusted must not gate new services in.

**What the operator does.** Repair the offending `scope_registry` /
`scope_registry_service` rows in Postgres **manually** (the error log names the
trip; the corrupt value is in the log). Once the rows are consistent, the nak'd
commands **redeliver and succeed on their own** — **no restart of Identity, the
declarants, or anything else is needed**. The 5s nak cadence is what makes
recovery automatic after the manual fix: redelivery, not a reboot, is the
recovery mechanism.

## Confirmations

`accepted` / `rejected` are `IntegrationEvent` envelopes on
`identity.evt.service_scope.{accepted,rejected}.v1`:

- `correlation_id` **echoes** the command's `metadata.correlation_id` (the
  declarant correlates the reply to its command on it);
- `causation_id` is the command's `command_id` (the confirmation is the direct
  effect of that command);
- the `actor` **echoes** the command's actor — the confirmation is caused by the
  declarant's command, so the same actor keeps the causal chain honest (there is
  no separate Identity machine identity to attribute it to in this slice).

The publisher **always re-emits**, including for an idempotent re-declare (which
the domain judges `Accepted` with an empty result): a rebooted replica
re-declaring its scopes must receive its `accepted`, so its readiness is never
stuck waiting on a confirmation a "nothing changed, skip" optimization would have
swallowed.

## Domain events

The domain command returns `CommandResult.events` (granular `RegistryEvent`s).
**This slice logs them and does not publish them on the domain bus** — no
consumer in this slice needs them (the integration-bus `accepted`/`rejected`
confirmations are what the declarant binds to), and persisting current-state rows
is the state-stored model's source of truth. This is an honest, documented
decision, not a pretend-dispatch: a later slice that needs a domain-bus fan-out
(e.g. a grant-admin projection) will lower them to a `RawEvent` envelope on the
unprefixed domain subject convention and publish them then.

## Outbound KV projection

Deliberately **skipped** in this slice — Postgres remains the source of truth. A
future enhancement may project the registry into a NATS KV bucket for low-latency
scope lookups; it is not needed now.

## Usage

```rust,no_run
use std::sync::Arc;
use br_identity_app::{
    ConfirmationPublisher, ScopeDeclarationPipeline, ScopeRegistryRepository,
    migrate, run_scope_declarations,
};
use br_core_integration::NatsIntegrationPublisher;
use br_util_axum_readiness::ReadinessHandle;

# async fn boot(
#     owner_pool: sqlx::PgPool,
#     app_pool: sqlx::PgPool,
#     jetstream: async_nats::jetstream::Context,
#     readiness: ReadinessHandle, // started not_ready, shared with /readyz
# ) -> Result<(), Box<dyn std::error::Error>> {
// On boot (owner/migration pool): apply the schema, explicitly.
migrate(&owner_pool).await?;
// ... ensure_app_role + grant_app_access (br-util-postgres) ...

// Composition root: assemble the pipeline from the app pool + publisher.
let publisher = Arc::new(NatsIntegrationPublisher::new(jetstream.clone()));
let repository = ScopeRegistryRepository::new(app_pool);
let confirmations = ConfirmationPublisher::new(publisher);
let pipeline = Arc::new(ScopeDeclarationPipeline::new(repository, confirmations));

// Bind the PRE-DECLARED stream + durable consumer and run (parks at zero CPU).
//
// CALLER CONTRACT: this future is the declaration path. The composing service
// MUST observe it (and the on_permanent_failure callback) and wire BOTH into its
// readiness gate — spawning and dropping the handle silently loses the fail-loud
// property (the consumer dies, the Err is discarded, the service serves on with
// a dead path). Select on it alongside the server, mark readiness DOWN if it
// resolves or if the corrupt-store callback fires.
run_scope_declarations(
    &jetstream,
    "IDENTITY_CMD",            // pre-declared stream
    "scope_declare_worker",    // pre-declared durable consumer
    pipeline,
    |poison| tracing::error!(error = %poison, "poison declare payload termed"),
    // Corrupt store at rest (see "Corrupt store (operator remediation)"): drop
    // readiness / raise an alert. Idempotent — it may fire on every redelivery
    // while the store stays corrupt.
    move |err| readiness.set_not_ready(format!("scope registry store corrupt: {err}")),
)
.await?;
# Ok(())
# }
```

## Install

```toml
[dependencies]
br-identity-app = { git = "https://github.com/BotResources/br-rust-common", package = "br-identity-app", tag = "v0.9.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
