# Changelog

All notable changes to the `br-rust-common` workspace are documented here. The
format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

From `0.8.0` on, every crate in the workspace shares a single version, this file
is the single changelog, and the workspace ships under one git tag (`vX.Y.Z`).
Earlier per-crate versions and their changelogs were consolidated into this
release; they remain reachable through the historical per-crate tags
(`<crate>-vX.Y.Z`).

## [Unreleased]

## [1.0.0] — 2026-06-17

### Added

- **New crate `br-util-nats-fabric` — the Project NATS Fabric API.** The single,
  restricted, typed application-facing way a BR service touches NATS; it owns all
  `async_nats` coupling. Two surfaces: **integration messaging** over a fixed v1
  grammar (`integration.cmd.{receiver}.{aggregate}.{verb}.v{N}` /
  `integration.evt.{producer}.{aggregate}.{fact}.v{N}`) on fixed streams
  (`INTEGRATION_CMD` / `INTEGRATION_EVT`), and the **Published-Language KV**
  generic mechanics over the fixed `PUBLISHED_LANGUAGE` bucket. The `Fabric`
  handle is built from an existing JetStream `Context` and **never provisions**
  (no stream/bucket creation; binds and fails loud when a declared object is
  absent). Callers supply only business coordinates — validated `Bc` /
  `Aggregate` / `Verb` / `PastFact` newtypes and `CommandCoords` / `EventCoords`
  — never stream names, the `integration` prefix, or the grammar; there is no
  freestyle string subject builder. Command/event durable consumers verify the
  durable's configured filter matches the requested coordinates
  (`FabricError::FilterMismatch`) so a misconfigured durable cannot silently
  widen its delivery. Includes the correlated awaiter, a transactional outbox
  (feature `outbox`) whose record destination is a typed `EventCoords` rendered
  at publish time, and generic Published-Language publisher (semantic upsert /
  retract / reconcile / drift-repair) and consumer (bootstrap scan + watch with
  consumer-selected prefixes, a copy-filter `Fn(&V) -> bool` that orphan-deletes
  pass→fail entries, and a caller-owned lossless projection sink) mechanics, both
  constructed via `open(&fabric)` — the raw `async_nats` KV `Store` is never
  handed to a caller, so every key path goes through a validated `KvKey` /
  `KvPrefix`.
  Builds on `br-core-integration` (envelopes, coordinates, the pure outbox state
  machine); it is now the **sole** owner of all NATS transport.

- **`br-util-nats-fabric` — typed single-key Published-Language read
  (`PublishedLanguageReader<V>`).** `PublishedLanguageReader::<V>::open(&fabric)
  .get(&key) -> Result<Option<V>, FabricError>` reads exactly one entry by its
  validated `KvKey` — for the consumer that needs one known key (e.g. the
  directory manifest `identity/_meta`) rather than a prefix scan with a capturing
  sink. **Exact-key** (a prefix sibling is never matched), **fail-closed decode**
  (an undecodable value is an explicit `FabricError::Decode` naming the key, never
  a silent `None`), `Ok(None)` only for a genuinely absent key, and
  **bind-existing** (the fixed `PUBLISHED_LANGUAGE` bucket, fail loud if absent;
  no provisioning) — like the rest of Surface 2, no raw `Store` escape hatch.

- **`br-core-directory` — `PublishedServiceAccount` directory DTO + the
  `service_accounts` entity, key prefix and builders.** A separate, minimal
  concrete DTO (single typed core field `name`, the rest in `extensions`) for
  the identity Published Language's service-account roster. `PublishedEntity`
  gains a concrete `ServiceAccounts` variant (not `Other("service_accounts")`)
  with a `DirectoryMeta::publishes_service_accounts()` accessor, and the frozen
  KV-key surface gains `SERVICE_ACCOUNTS_KEY_PREFIX`
  (`identity/service_accounts/<id>`), `service_account_kv_key` and
  `service_account_id_from_kv_key`. Groups stay user-only; no
  `PublishedPrincipal` is introduced.

- **`br-util-directory` — consumer-owned roster control + service-account
  projection.** The consumer side gains a `DirectoryConsumerConfig` with three
  seams (defaults preserve the prior behavior): `extract_user_extensions(Fn(&
  PublishedUser) -> PersistedExtensions)` selects the extension payload persisted
  into a new `jsonb extensions` column on `known_users` and exposed on
  `KnownUser` / `DirectorySnapshot::user_extensions` (**default keeps nothing**);
  `filter_users(Fn(&PublishedUser) -> bool)` scopes which users are copied, with
  a user flipping pass→fail orphan-deleted on the next reconcile / watch
  (**default keep-all**); and `scope(ConsumptionScope)` declares `UsersOnly` vs
  `UsersAndGroups` (**default `UsersAndGroups`**) independent of the producer
  manifest — `UsersOnly` projects and watches only `known_users`, touching no
  group tables. `PublishedServiceAccount` is now projected into a new
  `known_service_accounts` table (mirroring `known_users`) and exposed via
  `DirectorySnapshot::resolve_service_account`, honored by reconcile / watch when
  the manifest declares `service_accounts`. `DirectoryPublisher` gains
  `publish_service_account` / `retract_service_account` and the `DirectorySource`
  seam a `desired_service_accounts` method (default empty).
- **`br-core-integration` now owns the transport-independent integration
  coordinate types.** The validated newtypes `Bc` / `Aggregate` / `Verb` /
  `PastFact`, the `CommandCoords` / `EventCoords` structs and their `CoordError`
  moved here from `br-util-nats-fabric`: they are contract types (segment
  validation, no NATS coupling), so a core contract crate can build on them
  without a core→util dependency. `br-util-nats-fabric` re-exports them and keeps
  only the NATS-specific rendering (the `integration.…` subject assembly, now
  also exposed as `command_subject` / `event_subject`), parsing, stream
  constants and transport. `br-identity-domain` gains `RejectedIdentity`
  (`Service(ServiceKey) | Unrepresentable { raw }`) so a declaration with an
  invalid manifest key produces a typed rejection identity instead of an
  unwrap-or-default placeholder.

### Changed

- **`br-core-directory`: `PublishedUser` omits absent names on re-serialization.**
  `first_name` / `last_name` now carry `skip_serializing_if = "Option::is_none"`,
  so a wire with a name absent round-trips identically (`{"email":"x"}` →
  `{"email":"x"}`) instead of re-emitting explicit `first_name: null` /
  `last_name: null`. `email` stays always present. A wire-shape refinement on the
  read-and-written `KV_PUBLISHED_LANGUAGE` DTO; absent and explicit-null remain
  equivalent on deserialization.
- **BREAKING — `br-util-graphql`: `SubscriptionPayload` now requires a
  caller-supplied type name.** The signature is `SubscriptionPayload<N, E, T>`
  where `N: PayloadName` carries `const NAME: &'static str`. Previously the
  GraphQL type name was derived from the entity `T` alone, so the same entity
  paired with two different event unions silently produced the same SDL type
  name and collided. The name is now explicit and per-pairing, so a collision is
  unrepresentable. `PayloadName` is exported. The list-entity name fix from
  0.11.1 is subsumed: the caller names the payload directly, list or scalar.
- **BREAKING — `br-core-auth`: `Passport` variant fields are now private and
  `claims` is a `PassportClaims` newtype.** `Passport::{Human,Service}` can no
  longer be built from raw struct literals by outside crates; construct through
  the new canonical constructors `Passport::human(user_id, is_super_admin,
  is_active, auth_method, impersonator, claims)` and
  `Passport::service(service_account_id, claims)`, plus the `with_impersonator`
  helper. `claims()` now returns `&PassportClaims` (was `&serde_json::Value`).
  The new `PassportClaims` newtype wraps a JSON **object** map (private inner),
  serializes as an object, and **rejects any non-object** (null/array/scalar) on
  deserialization — so a non-canonical passport (e.g. `claims: null`) is now
  unrepresentable, not merely rejected on the wire. New read accessors
  `user_id()` / `service_account_id()` (both `Option<Uuid>`). The valid-passport
  wire format and the `X-Passport` base64 codec are unchanged.
- **BREAKING — `br-core-events`: `DomainEvent.metadata` is now the typed
  `EventMetadata`** instead of `serde_json::Value`, and `DomainEvent::new` takes
  `metadata: EventMetadata`. An event can no longer persist a malformed metadata
  bag. The JSON wire shape is unchanged (`EventMetadata` already serialized to
  the same flat `actor_id`/`actor_kind`/`correlation_id` form).
- **BREAKING — `br-core-directory`: the `PublishedUser` / `PublishedGroup` /
  `PublishedServiceAccount` `extensions` bag is now private behind a validating
  constructor.** Each DTO exposes `new(...)` (and a public `extensions()`
  accessor) and rejects, with the new `DirectoryError`, an `extensions` map whose
  key shadows a reserved core field (`email` / `first_name` / `last_name` for
  users, `name` / `member_ids` for groups, `name` for service accounts; surfaced
  as the `PUBLISHED_*_RESERVED_KEYS` constants). `Deserialize` is hand-written and
  routes through the same constructor, so the wire — which is both read **and**
  written for `KV_PUBLISHED_LANGUAGE` — is fail closed against a shadowing key.
  Direct struct construction of these DTOs is no longer possible; use `new`.
- **`br-util-broadcast`: `EventBus::new` now rejects a zero capacity.** It
  panics with a precise message (a zero-capacity broadcast channel buffers
  nothing and would drop every event); capacity is a composition-root config
  value, so a zero is a programming error rather than a runtime condition.
- **BREAKING — `br-util-directory` is re-expressed on top of `br-util-nats-fabric`.**
  Its own KV engine is **deleted** — the public `KvOp` / `reconcile_entries` and
  every raw `async_nats` KV `Store` put/delete/observe/apply are gone; the kit
  now holds only the directory *meaning* (keys, DTOs, schema, recompose) over the
  fabric's generic publisher/consumer. `DirectoryPublisher::new(kv: Store)` /
  `DirectoryProjector::new(kv: Store, pool)` are replaced by
  `DirectoryPublisher::open(&fabric)` (async) and
  `DirectoryProjector::new(fabric, pool)` / `with_config(fabric, pool, config)`.
  The projector's imperative `apply_user` / `remove_user` / `apply_group` /
  `remove_group` are removed — incremental projection is the fabric `watch()`.
  `DirectoryError` drops the `Kv` / `Wire` string variants in favor of
  `Fabric(FabricError)` / `KvKey(KvKeyError)`, and gains `ManifestAbsent`.
- **`br-util-directory` — `read_manifest` reads `identity/_meta` via the new
  single-key `PublishedLanguageReader::get`** instead of opening a prefix-scoped
  consumer with a capturing one-shot sink and filtering on exact-key equality.
  This drops the workaround and closes the prefix over-matching it carried (the
  old `identity/_meta` *prefix* would also match `identity/_metadata`); the
  observable fail-closed / absent-as-`ManifestAbsent` behavior is unchanged.
- **BREAKING — the scope-declaration slice runs over the NATS Fabric.** The
  handshake and the Identity receiver no longer touch the legacy
  `br-core-integration` transport (`NatsIntegrationPublisher`,
  `DurableConsumer::bind`, `CorrelatedAwaiter::create`, the freestyle
  `integration_subject` builder) — they use `br-util-nats-fabric` over the fixed
  `INTEGRATION_CMD` / `INTEGRATION_EVT` streams and the v1 grammar. The contract
  subjects move from the old bc-prefixed grammar
  (`identity.cmd.service_scope.declare.v1`) to the Fabric grammar
  (`integration.cmd.identity.service_scope.declare.v1`,
  `integration.evt.identity.service_scope.{accepted,rejected}.v1`).
  - **`br-scope-declaration-contract`** replaces the freestyle subject builders
    (`command_subject` / `event_subject` / `accepted_subject` /
    `rejected_subject`) with the typed `declare_command_coords()` /
    `accepted_event_coords()` / `rejected_event_coords()` returning the core
    `CommandCoords` / `EventCoords`. Adds `UNREPRESENTABLE_SERVICE`.
  - **`br-util-scope-declaration`**: `declare_scopes` now takes `&Fabric` (not a
    raw JetStream `Context`) and awaits the two confirmation facts via the
    Fabric correlated awaiter; `ScopeDeclarationConfig` drops `stream_name` (the
    standard flow never names a stream). Readiness gating, timeout / correlation
    / re-publish policy and the disabled-mode no-publish path are unchanged.
  - **`br-util-nats-fabric`**: adds `Fabric::await_events(&[&EventCoords])` (the
    correlated awaiter over more than one reply fact) and the public
    `command_subject` / `event_subject` renderers.
  - **`br-identity-app`**: `run_scope_declarations` takes `&Fabric` + the declare
    `CommandCoords` + a durable name (the Fabric binds `INTEGRATION_CMD` and
    verifies the durable filter so a misconfigured durable cannot widen
    delivery); confirmations publish via the Fabric. `ConfirmationPublisher` /
    `ScopeDeclarationPipeline` drop their `IntegrationPublisher` generic and hold
    a concrete `Fabric`. `AppError::Publish` now wraps `FabricError`.
- **BREAKING — `br-identity-domain`: registry-hydration and rejection audit
  fixes.** `ScopeRegistry::hydrate` now rejects a duplicate `ServiceKey` even
  when the services' scopes do not overlap
  (`RegistryHydrationError::DuplicateService`). `DeclarationOutcome::Rejected`
  carries a typed `RejectedIdentity` (`Service(ServiceKey) | Unrepresentable {
  raw }`): the app maps `Unrepresentable` to the explicit `UNREPRESENTABLE_SERVICE`
  sentinel rather than the previous silent `unwrap_or_else("unknown")`.
  Re-declaration of an already-owned scope is documented as **label/description
  immutable for v1** (a re-declare only touches `last_seen_at`; the existing
  no-op behavior is now the explicit contract).

### Removed

- **BREAKING — `br-core-values`: removed the `ValueError::Unknown { code }`
  catch-all variant.** A non-canonical `code` on the wire now **fails
  deserialization** (`unknown variant`) instead of degrading to a publicly
  constructible `Unknown` state. Every `ValueError` is one of the fixed
  canonical codes.
- **BREAKING — `br-core-events`: removed `RawEvent`** (and its export). It was a
  pre-persistence producer-side shape; aggregates lower their facts straight into
  a `DomainEvent` envelope.
- **BREAKING — `br-util-postgres`: removed `set_rls_context`.** The exact shape
  of the `app.*` RLS session variables (which fields, which names) is
  project-specific — it depends on a service's Passport claims and its policy
  model — so it does not belong in the shared lib. Each service now injects its
  own transaction-local RLS context with `set_config(..., true)`; this crate
  keeps the pool/TLS/role/grant wiring underneath. `br-core-auth` and `tracing`
  are no longer dependencies of `br-util-postgres`.
- **BREAKING — `br-util-postgres`: removed the deprecated `TRUSTED_HOSTS`
  environment-variable fallback.** `resolve_trusted_network_hosts` now reads
  **only** `TRUSTED_NETWORK_HOSTS`; `TRUSTED_HOSTS` (deprecated since 0.6.0) is
  no longer honored and no longer warns. Services still setting the legacy name
  must rename it.
- **BREAKING — `br-core-integration` reduced to pure, transport-independent
  contracts; all NATS transport removed.** Now that `br-util-nats-fabric` owns
  every NATS transport path and the scope-declaration + identity slices have
  migrated onto it, the crate's `async_nats`-coupled transport is gone:
  `NatsIntegrationPublisher`, the `IntegrationPublisher` / `IntegrationPublisherExt`
  traits and `NoopIntegrationPublisher`, `DurableConsumer` / `Delivery`,
  `CorrelatedAwaiter` / `CorrelatedMatch`, the freestyle `integration_subject` /
  `MessageKind` / `SubjectError` subject builder, `verify_consumer`, the
  transport-coupled `IntegrationError` / `PublishErrorKind` / `ConsumeErrorKind`
  error types, and the `outbox` feature's Postgres store + relay
  (`OutboxStore` / `OutboxRelay` / `RelayPolicy` / `RelayHealth` / `OutboxRecord` /
  `stage` / `classify_failure` / `FailureClass`) — all removed. The crate **no
  longer depends on `async-nats`** (nor on `sqlx`, `tokio`, `async-trait`,
  `futures-util`); the `outbox` feature is gone. What remains is pure: the message
  coordinates (`Bc` / `Aggregate` / `Verb` / `PastFact`, `CommandCoords` /
  `EventCoords`, `CoordError`), the `IntegrationEvent` / `IntegrationCommand`
  envelopes, `MessageOutcome` (minus its `async_nats::AckKind` conversion, now a
  Fabric-local function), and the pure outbox state machine (`OutboxStatus` /
  `Transition` / `next_after_attempt` / `retry_backoff`) the Fabric reuses.

### Other

- **`br-test-support` is marked `publish = false`** so it can never be released
  to a registry or mistaken for a normal public surface (it was already
  dev-dependency-only and workspace-internal).
- **`br-util-observability` README** documents that the crate is a BR platform
  observability **convention** (opinionated Axum + `tracing`-JSON + Prometheus +
  liveness + process/HTTP collectors), not a vendor-neutral utility, and that
  `MetricsHandle::prometheus() -> &PrometheusHandle` is an **intentional** part
  of the public contract.

### Fixed

- **`br-util-directory`: a missing identity manifest no longer purges the local
  roster (PII-purge fix).** Previously an absent `identity/_meta` was treated as
  an empty roster, so a consumer that merely booted ahead of identity's first
  reconcile orphan-deleted every `known_*` row. `reconcile()` / `watch()` now
  **fail closed** with `DirectoryError::ManifestAbsent` and leave the projection
  untouched — a degraded/unready condition, never a delete-all.
- **`br-util-directory`: membership projection now converges regardless of
  user-vs-group projection order.** `known_user_group.user_id` carries **no**
  foreign key, and the group sink records **every** `(group_id, user_id)` pair
  straight from the group's `member_ids` (delete-then-insert per group), dropping
  the prior member-existence guard. The user, group and service-account watches
  are independent streams with no inter-entity re-trigger, so a group could
  project before one of its members' user entry (watch reordering, or a member
  published after the group); the FK + existence guard silently skipped that
  membership row and never re-projected the group when the user later arrived, so
  `is_member` stayed wrong. Under this read-only roster a membership referencing a
  not-(yet/ever)-projected user is **legitimate**, not corruption — `is_member` is
  correct from the group projection, while `resolve_user` returns `None` for a
  filtered/not-yet-projected user (the expected scoped behavior). Group deletion
  still CASCADEs the junction via the retained `group_id` FK (#69's "FK **or**
  deterministic orphan cleanup").
- Correct stale `MIT` license references to `Apache-2.0` in `CONTRIBUTING.md`
  and the `br-test-support` README (the workspace relicensed to Apache-2.0; the
  `LICENSE` file and crate manifests were already correct).

## [0.11.1] — 2026-06-15

### Fixed

- **`br-util-graphql` — `SubscriptionPayload<E, T>` now derives a valid GraphQL
  type name for a list entity `T`.** Previously the name was a verbatim
  concatenation of `T::type_name()`, so a list entity (`Vec<_>`, whose
  `type_name()` is `[Inner!]`) produced the invalid SDL identifier
  `[Inner!]SubscriptionPayload`, which `grafbase compose` rejects — taking the
  whole subgraph (and therefore the gateway) down. The wrapper type name now
  strips the list/non-null punctuation and encodes list-ness as a `List` suffix
  (`[OrgMembership!]` → `OrgMembershipListSubscriptionPayload`), so it can never
  collide with the scalar payload of the same element (`UserSubscriptionPayload`
  vs `UserListSubscriptionPayload`). Only the wrapper's type name changes; the
  `entity` field stays a list, so the wire data shape is unchanged.

## [0.11.0] — 2026-06-14

### Added

- **`br-util-directory` — publisher + consumer kit for the identity Published
  Language.** A single `util`-tier crate built on `br-core-directory`,
  **feature-gated to honor the real dependency asymmetry**: `default = []`;
  `publisher` touches NATS KV only (no Postgres); `consumer` additionally pulls
  `br-util-postgres` + `sqlx` for the KV→PG projection. The kit **never
  auto-creates the KV bucket or the PG schema** — it takes an already-bound
  `kv::Store` / `PgPool` and fails loud if infra is absent. Each side
  reconciles by **orphan-delete**, never wipe (the PII-deletion guarantee).
  - *Publisher* — the project supplies its source of truth through the
    `DirectorySource` seam (`manifest` + `desired_users` + `desired_groups`);
    `DirectoryPublisher` provides the mechanism: `reconcile` (whole-bucket diff
    + minimal put/delete + `_meta` write, degrading groups when the manifest
    drops them) and the incremental `publish_user` / `retract_user` /
    `publish_group` / `retract_group` / `write_meta`. The minimal diff is the
    pure `reconcile_entries(desired, observed) -> Vec<KvOp>`.
  - *Consumer* — `connect_pool` (a TLS-validated pool via
    `br_util_postgres::init_pool`), `migrate` (`known_users` / `known_groups` /
    the `known_user_group` junction) and `DirectoryProjector`, the KV→PG
    projector (`reconcile` on boot + incremental `apply_*` / `remove_*`). The
    **denormalized KV group wire (`member_ids`) is recomposed into the
    normalized junction** via the pure `member_rows`, each group upsert applied
    in one transaction. Typed readers **carry the key-derived id** —
    `resolve_user` / `is_member` / `group_name` over `DirectorySnapshot` — and
    **auto-degrade** (group readers return `None` / `false` when the manifest
    omits `groups`). Pure logic (diff, orphan set, recompose, reader resolution
    + auto-degrade) is unit-tested here; the real-PG / real-NATS Px/Cx suites
    are out of scope (br-e2e-harness, a later work unit).
- **`br-core-directory` — frozen read contract for the identity Published
  Language.** Pure `core`-tier serde DTOs for the identity directory roster
  published over NATS KV (display / enumeration, never authZ): `PublishedUser`
  (typed core `email` / `first_name` / `last_name` + a flattened `extensions`
  bag), `PublishedGroup` (typed core `name` / `member_ids` with
  `has_member` + `extensions`), the `DirectoryMeta` (`identity/_meta`) manifest
  with auto-degrade (`publishes_users` / `publishes_groups`), and the frozen KV
  key conventions (`USERS_KEY_PREFIX` / `GROUPS_KEY_PREFIX` / `META_KEY`,
  `user_kv_key` / `group_kv_key` + the reverse `*_id_from_kv_key` parsers). The
  wire is **extracted and frozen from the live, already-consumed
  be-botresources Published Language**, not invented. Core + extension model
  like the Passport claims bag: the contract binds the kernel and stays
  policy-free, while project-specific fields (`locale`, …) ride opaquely in the
  flattened `extensions` bag — the contract never names them and a consumer
  reads them entirely on its own side. No `sqlx` / `async-nats` deps so it
  imports cleanly as a wire oracle. The publisher / consumer kits and the Px/Cx
  conformance suites are out of scope (other work units).
- **`br-util-graphql` — localized output bridge (`GqlLocalized` / `GqlLocalizedEntry`).**
  The crate now ships the **output** half of the localized-value bridge to match the
  existing input half (`GqlLocalizedInput`). Two `#[derive(SimpleObject)]` types and a
  converter `GqlLocalized::from_localized::<F, L: GqlLocale>(&Localized<F, L>)`, both
  format-agnostic (`F` = `Markdown`/`Html`/`PlainText`) and locale-agnostic (generic
  `L`), let a subgraph return a `Localized<F, L>` in a GraphQL response without
  hand-rolling a local `SimpleObject`. The canonical-locale field is named
  **`primaryLocale`** (a wire locale code), never `primary` — it holds a code, not the
  text. `entries` carries **every** locale, the primary included.
- **`br-core-auth` — `PassportBuilder` behind the `test-support` feature.** A
  fluent builder for forging a `Passport` (`.user_id() .super_admin() .active()
  .pat() .impersonator() .claim() .claims()` + `.build()` / `.build_service()`),
  co-located with the type it builds so it tracks every field change with zero
  drift. Policy-free: claim keys are set through the generic `claim` / `claims`,
  never baked in. Gated behind `feature = "test-support"` so it never reaches a
  production binary; enable it as a dev-dependency. Promotes the builder that
  lived downstream in `br-test-harness`.
- **`br-core-auth` — typed scopes claim binding (`Passport` ↔ `ScopeKey`).**
  `pub const SCOPES_CLAIM_KEY = "scopes"`, `Passport::scopes() -> Vec<ScopeKey>`
  and `Passport::has_scope(&ScopeKey) -> bool`, plus a re-export of
  `br_core_scope::ScopeKey`. The scope grant carried in the Passport is now a
  typed platform contract end-to-end (declared as `ScopeKey`, granted as
  `ScopeKey`, read as `ScopeKey`), replacing the per-service
  `claim::<Vec<String>>("scopes")` convention. Serialized shape: a JSON array of
  scope-key strings under the `scopes` claim. `scopes()` skips malformed entries
  and `has_scope` is fail-closed — a bad claim entry never widens access.
  `br-core-auth` gains a (verified-acyclic) dependency on `br-core-scope`.

### Changed (breaking)

- **`br-util-graphql`: `GqlLocale` gains a required method `fn as_wire(&self) -> &str`.**
  The trait now owns **both directions** of the wire↔locale mapping: `from_wire`
  (string → locale) and `as_wire` (locale → string), the latter needed by
  `GqlLocalized::from_localized` to emit a locale code on the wire. This is the
  symmetric design (one trait, both directions) chosen over bounding the converter on
  `L: AsRef<str>` — `AsRef<str>` would force every product locale to expose a `&str`
  view that may not equal its wire code, whereas `as_wire` is the explicit, dedicated
  inverse of `from_wire` and cannot diverge from it. Adding a required trait method is a
  breaking change for external impls, but the lib has **no non-test `GqlLocale` impls**
  and a `0.x → 0.x` minor bump may break per Cargo's semver rules; a product impl adds
  one `as_wire` match arm per locale. (Note: `cargo-semver-checks` runs **0 checks** on
  this crate because its entire surface sits behind a single crate-root
  `#![cfg(feature = "graphql")]`, so the tool produces an empty comparison surface — its
  "no semver update required" line reflects *nothing checked*, not *no break*; the
  breaking change above is asserted by inspection, not by the tool.)

### Changed

- **`br-core-values` — `Localized<F, L>` now trims leading/trailing whitespace
  from content at construction.** Every construction path normalizes each
  entry's content with `str::trim()`: `new`, `from_parts` (which covers both the
  `Deserialize` path and the `br-util-graphql` `GqlLocalizedInput::into_localized`
  bridge, since both route through it) and `set`. **Interior whitespace is
  preserved** — Markdown indentation, blank lines between paragraphs and
  code-block whitespace are semantic, so only the outer edges are stripped, never
  collapsed. This removes equality/dedup/wire-roundtrip drift where two logically
  equal contents differed only by a trailing newline. Whitespace-only content
  trims to the empty string, which stays allowed (required-ness is a domain seam,
  not the value object's job). **This is a behavior change with no public-API
  signature change** — no type, function signature or serde shape moved, so
  `cargo-semver-checks` does not (and should not) flag it; its silence here is
  correct, not evidence the behavior is unchanged.
- Relicensed from MIT to Apache-2.0.

### Fixed

- **Workspace internal version pins realigned to `0.11.0`.** Opening the
  `0.11.0` integration branch bumped `[workspace.package] version` but left the
  `[workspace.dependencies]` path-dep pins at `version = "0.10.0"`, so every
  internal crate failed to resolve (`requirement br-core-* = "^0.10.0"` did not
  match the `0.11.0` candidate). Bumped all internal pins to `0.11.0`.

## [0.10.0] — 2026-06-13

### Fixed

- **`br-core-integration` / `br-util-scope-declaration` — scope-declaration boot
  no longer eats ~10s of dead time on the happy path.** `CorrelatedAwaiter` waited
  for the correlated reply over a freshly-created `DeliverPolicy::New` ephemeral
  JetStream pull consumer. A reply that arrived *during the first await window* was
  not surfaced on that window's `messages()` stream (a pull-request establishment
  race on the new consumer); it was only drained on the next loop pass, after a
  full `wait_timeout` elapsed and `declare_scopes` re-published. Every
  scope-declaring service therefore paid one `wait_timeout` (default 10s) of dead
  time before readiness flipped UP, even when Identity replied in under a second.
  The awaiter now awaits over a **core NATS push subscription** opened (before the
  command is published) on the confirmation subjects, so a reply landing inside the
  first window is delivered within it — the establishment race is structurally gone
  and the subscription parks at zero CPU between waits with no pull requests. The
  declare command stays on JetStream (durable); a JetStream publish also reaches
  core subscribers on the same subject. The fail-loud-on-missing-stream contract is
  unchanged: `create` still asserts the declared stream exists and returns
  `ConsumeErrorKind::NoStream` if absent.

### Changed (breaking)

- **`br-core-integration`: `AwaiterConfig` and `CorrelatedAwaiter::create_with`
  removed.** `AwaiterConfig` tuned only the ephemeral consumer's
  `inactive_threshold`, a JetStream-consumer concept with no meaning for a core
  push subscription. `CorrelatedAwaiter::create(jetstream, stream_name,
  filter_subjects)` is now the sole constructor; its signature and
  `await_correlation` / `CorrelatedMatch` are unchanged.
- **`br-util-scope-declaration`: `ScopeDeclarationConfig.awaiter` field removed.**
  It carried the now-deleted `AwaiterConfig`. `ScopeDeclarationConfig` still tunes
  `enabled`, `stream_name`, and `wait_timeout`; construct it with `enabled(..)` /
  `disabled(..)` as before.

## [0.9.0] — 2026-06-13

### Added

- **`br-util-graphql` — `Affordance.params`.** `Affordance` gains an optional
  `params: Option<Json<BTreeMap<String, String>>>` field: a structured,
  codes-keyed map (same Rust type and builder ergonomics as `EdgeError`'s
  `params`) that attaches data keyed to `reason_code`, exposed on the wire as a
  nullable GraphQL `JSON` scalar (`params: JSON`). Attach it with the chainable
  `Affordance::block(action, reason).with_param(key, value)` / `.with_params(...)`;
  `allow` / `block` leave it unset (renders `null`, existing outputs unchanged).
  The map is codes, never user-facing prose — human copy and i18n stay at the edge.

### Changed

- **`br-util-graphql` — `Affordance` is now `#[non_exhaustive]`** (breaking). This
  is the breaking change that justifies the minor bump: external crates can no
  longer build an `Affordance` by struct literal — construct via
  `Affordance::allow` / `Affordance::block` (then `.with_param` / `.with_params`),
  the only supported path. In exchange, every future field addition to
  `Affordance` becomes non-breaking. In-crate literal construction is unaffected.

## [0.8.0] — 2026-06-13

### Changed

- **Unified versioning.** Every crate now inherits `version.workspace = true`
  from `[workspace.package] version = "0.8.0"`; internal `[workspace.dependencies]`
  pin `version = "0.8.0"`. The repo releases as one git tag `v0.8.0` instead of
  per-crate `<crate>-vX.Y.Z` tags. README install snippets pin `tag = "v0.8.0"`
  with a `package = "<crate>"` selector.
- **CI moved to the unified scheme.** `release-tags.yml` emits one `vX.Y.Z` tag
  and release from the workspace version; `check-changelog.sh` validates the
  single workspace version against this root changelog; `check-readme-pins.sh`
  validates the unified `tag = "vX.Y.Z"` pin form; the `semver` job baselines
  every crate against the single previous `vX.Y.Z` tag and skips crates that did
  not exist at that baseline.
- **One name for the message metadata type.** `br-core-integration` re-exports
  `br_core_events::EventMetadata` as `EventMetadata` (the `MessageMetadata` alias
  is removed). Consumers reference `EventMetadata`.
- **`br-identity-app` pipeline returns the domain outcome.** The application-layer
  `HandledOutcome` enum is removed; `ScopeDeclarationPipeline::handle` returns the
  domain `br_identity_domain::DeclarationOutcome` directly.

### Added

- **`br-test-support`** — dev-only shared Postgres e2e test helpers (role / pool /
  name primitives), consumed as a path dev-dependency, never a normal dependency.
- **`br-scope-declaration-contract`** — single source of the identity service-scope
  declaration wire coordinates (`bc` / `aggregate` / `version` / command name) plus
  subject and command/event-type helpers, shared by the `br-identity-app` publisher
  and the `br-util-scope-declaration` handshake. The duplicated local constants are
  removed; the canonical wire-string assertion lives here.
- **`Passport::to_actor`** — bridges `br-core-auth`'s `Passport` to
  `br_core_kernel::Actor` (`Human → Actor::Human(UserId)`,
  `Service → Actor::Service(ServiceAccountId)`). `br-core-auth` now depends on
  `br-core-kernel`.

### Removed

- Per-crate `CHANGELOG.md` files (consolidated into this root changelog).
- The `HandledOutcome` enum and the `MessageMetadata` alias.
