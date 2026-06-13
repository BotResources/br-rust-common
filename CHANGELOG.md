# Changelog

All notable changes to the `br-rust-common` workspace are documented here. The
format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

From `0.8.0` on, every crate in the workspace shares a single version, this file
is the single changelog, and the workspace ships under one git tag (`vX.Y.Z`).
Earlier per-crate versions and their changelogs were consolidated into this
release; they remain reachable through the historical per-crate tags
(`<crate>-vX.Y.Z`).

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
