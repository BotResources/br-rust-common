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
