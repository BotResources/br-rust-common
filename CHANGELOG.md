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
  like the Passport claims bag: generic services bind the kernel,
  project-specific fields (`organization_id`, `locale`, `is_platform_member`, …)
  ride in `extensions`; tenancy stays an extension, never core. No `sqlx` /
  `async-nats` deps so it imports cleanly as a wire oracle. The publisher /
  consumer kits and the Px/Cx conformance suites are out of scope (other work
  units).

### Changed

- Relicensed from MIT to Apache-2.0.

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
