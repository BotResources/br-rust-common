# Changelog

All notable changes to the `br-rust-common` workspace are documented here. The
format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

From `0.8.0` on, every crate in the workspace shares a single version, this file
is the single changelog, and the workspace ships under one git tag (`vX.Y.Z`).
Earlier per-crate versions and their changelogs were consolidated into this
release; they remain reachable through the historical per-crate tags
(`<crate>-vX.Y.Z`).

## [Unreleased]

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
  pass→fail entries, and a caller-owned lossless projection sink) mechanics.
  Builds on `br-core-integration` (envelopes, the pure outbox state machine);
  the old `br-core-integration` transport APIs remain available until consumers
  migrate.

### Fixed

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
