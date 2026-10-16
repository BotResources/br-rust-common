# br-rust-common

> [!IMPORTANT]
> **This repository is maintained for BotResources and its authorized clients.**
> It is published under Apache-2.0 and made available read-only for visibility
> and dependency consumption. The Apache-2.0 license governs your rights to
> use, modify, and fork the code; the rest of this notice describes our
> operational stance, not a legal restriction.
>
> **We do not accept external pull requests, issues, or support requests.**
> Issues and Discussions are disabled. PRs from accounts that are not on the
> internal contributor allowlist will be closed without review. The GitHub
> fork button is disabled — you may still fork under Apache-2.0, but we
> provide no support outside the BR commercial relationship.
>
> - Clients with a commercial relationship: contact your BR account manager.
> - Security reports: see [SECURITY.md](SECURITY.md) (private email channel).
> - This is not a community-supported project. No support is provided through
>   GitHub.

Small, reusable Rust crates for [BotResources](https://botresources.ai) services.

## Catalog

| Crate | Tier | Description | Version | Changelog |
|---|---|---|---|---|
| `br-core-kernel` | core | Typed ID wrappers (`UserId`, `ServiceAccountId`) | `0.1.0` | [CHANGELOG](crates/br-core-kernel/CHANGELOG.md) |
| `br-core-auth` | core | `Passport` DTO, `X-Passport` header codec, PAT bearer-token contract | `0.1.0` | [CHANGELOG](crates/br-core-auth/CHANGELOG.md) |
| `br-core-events` | core | Shared event envelopes (`EventMetadata`, `DomainEvent`) | `0.1.0` | [CHANGELOG](crates/br-core-events/CHANGELOG.md) |
| `br-core-integration` | core | Transport-independent integration contracts: message coordinates, `IntegrationEvent`/`IntegrationCommand` envelopes, `MessageOutcome`, and the pure outbox state machine (no NATS, no DB) | `0.1.0` | [CHANGELOG](crates/br-core-integration/CHANGELOG.md) |
| `br-core-scope` | core | Scope self-declaration contract types (`ScopeKey`, `ScopeDeclaration`, declare/accepted/rejected payloads) | `0.1.0` | [CHANGELOG](crates/br-core-scope/CHANGELOG.md) |
| `br-core-values` | core | Universal value objects: `Localized<F, L>` text family + ISO `Money` / `Currency` / `CountryCode` | `0.1.0` | [CHANGELOG](crates/br-core-values/CHANGELOG.md) |
| `br-core-directory` | core | Frozen read contract for the identity Published Language: `PublishedUser` / `PublishedGroup` directory entries, the `identity/_meta` manifest and the KV key conventions | `0.1.0` | [CHANGELOG](crates/br-core-directory/CHANGELOG.md) |
| `br-scope-declaration-contract` | core | Single source of the identity service-scope declaration wire coordinates (bc/aggregate/version/command + subject helpers), shared by `br-identity-app` and `br-util-scope-declaration` | `0.1.0` | [CHANGELOG](crates/br-scope-declaration-contract/CHANGELOG.md) |
| `br-util-boot` | util | The boot environment of a service: the names it reads at boot (`ENVIRONMENT`, `PORT`, `HOST`, `DATABASE_URL`, `NATS_URL`) and `BootEnv`, one typed, validating reader that reports every problem at once and never logs a credential | `0.1.0` | [CHANGELOG](crates/br-util-boot/CHANGELOG.md) |
| `br-util-postgres` | util | Postgres pools, TLS, app role, GRANTs; owns the names `DATABASE_URL_OWNER`, `TRUSTED_NETWORK_HOSTS` | `0.1.1` | [CHANGELOG](crates/br-util-postgres/CHANGELOG.md) |
| `br-util-axum-auth` | util | Axum middleware that injects `Passport` from `X-Passport` | `0.1.0` | [CHANGELOG](crates/br-util-axum-auth/CHANGELOG.md) |
| `br-util-axum-readiness` | util | Readiness gate for HTTP services, served on `READINESS_PATH` (`/readyz`) | `0.1.0` | [CHANGELOG](crates/br-util-axum-readiness/CHANGELOG.md) |
| `br-util-broadcast` | util | In-process event bus (tokio broadcast) for post-commit fan-out of domain events to same-process GraphQL subscriptions; the API shape forbids publishing before the tx commits | `0.1.0` | [CHANGELOG](crates/br-util-broadcast/CHANGELOG.md) |
| `br-util-graphql` | util | GraphQL/REST edge kit: `ErrorCode` cross-service contract, `Affordance` / `MutationResult` / `Connection` / `SubscriptionPayload`, fallible `br-core-values` wrappers | `0.1.0` | [CHANGELOG](crates/br-util-graphql/CHANGELOG.md) |
| `br-util-nats-fabric` | util | The Project NATS Fabric API: coordinate-only publish/consume/await over the fixed `integration.…` subject grammar, the KV published-language projection, and the Postgres transactional outbox (store + subscribe-driven relay) | `0.1.0` | [CHANGELOG](crates/br-util-nats-fabric/CHANGELOG.md) |
| `br-util-directory` | util | Publisher and consumer kit for the identity Published Language over the NATS Fabric: reconcile and orphan-delete (publisher), KV to Postgres projector over the `known_*` tables (consumer) | `0.1.0` | [CHANGELOG](crates/br-util-directory/CHANGELOG.md) |
| `br-util-observability` | util | Boot-time observability: structured JSON logging, an always-200 liveness route on `LIVENESS_PATH` (`/livez`), the Prometheus exposition on `METRICS_PATH` (`/metrics`) | `0.1.0` | [CHANGELOG](crates/br-util-observability/CHANGELOG.md) |
| `br-util-scope-declaration` | util | Boot-time scope-declaration handshake helper (declare scopes to Identity, gate readiness on the confirmation) | `0.1.0` | [CHANGELOG](crates/br-util-scope-declaration/CHANGELOG.md) |
| `br-identity-domain` | bc | Identity bounded context, pure domain — scope-registration slice (`ScopeRegistry` aggregate, commands, events) | `0.1.0` | [CHANGELOG](crates/br-identity-domain/CHANGELOG.md) |
| `br-identity-app` | bc | Identity bounded context, application/adapter half — scope-registration slice (Postgres persistence, durable NATS consumer, `load → judge → save → dispatch` pipeline, confirmations) | `0.1.0` | [CHANGELOG](crates/br-identity-app/CHANGELOG.md) |
| `br-test-support` | dev | Dev-only shared Postgres e2e test helpers (role/pool/name primitives); a path dev-dependency, never a normal dependency | not released | - |

## Helm chart

| Chart | Type | Description | Docs | Changelog |
|---|---|---|---|---|
| `br-rust-common-chart` | library | Deployment topology and ops contract of a BotResources Rust service built on these crates: Deployment, Service, ServiceAccount, PDB, NetworkPolicy, and the render guards for what the crates read at boot (probe paths, `PORT`, the two-role Postgres DSNs, `TRUSTED_NETWORK_HOSTS`, `NATS_URL`) — every name a constant of the crates, the chart gated against it | [README](charts/br-rust-common-chart/README.md) | [CHANGELOG](charts/br-rust-common-chart/CHANGELOG.md) |

The chart has its own version line, independent of the crates' version, and is
published to `oci://ghcr.io/botresources/charts/br-rust-common-chart` by the
`chart-release` workflow when its `Chart.yaml` version changes on `main` or a
`release/**` branch.

The chart cannot import a Rust constant, so
[`tools/br-ops-contract`](tools/br-ops-contract/src/main.rs) (a CI tool, never
published) prints the names the crates own into
`charts/br-rust-common-chart/ci/ops-contract.json`; its test fails when that file
is stale, and the chart gate fails when the chart renders anything else.

## Architecture

- `core` — cross-cutting constraints, **no dependency on `util`**.
- `util` — optional technical wrappers; may depend on `core`.
- `bc` — a packaged bounded context (`*-domain` + `*-app`), reusable per project; builds on `core` / `util`.
- No `svc-*` or business logic in this repo. Each crate defines its own errors.

## Distribution

Not on crates.io. Every publishable crate is its own library: it has its own version, its own `CHANGELOG.md` and
is released by its own git tag `{crate}/v{version}`, starting at `0.1.0` (`br-test-support` is `publish = false` and
is never released). All 20 crates of the catalog are released at `0.1.0`, in five waves, each crate after the sibling
crates it depends on.

Release rule, enforced by `check-sibling-pins.sh`: a released crate pins each internal sibling it depends on by tag,
as `{ git = "https://github.com/BotResources/br-rust-common", tag = "<sibling>/v<version>", version = "<version>" }`,
with a root `[patch."https://github.com/BotResources/br-rust-common"]` to the local path of the sibling, so the
workspace holds one copy of each. Sibling dev-dependencies are exempt from the tag-pin rule (`check-sibling-pins.sh`
skips them, and a consumer's lockfile does not contain them).

`v1.4.0` is the legacy workspace tag: the last release of the whole workspace under one version, kept for the services
still on it (the tag stays, never rewritten). Moving from `v1.4.0` to per-crate pins means moving **all** the
`br-rust-common` crates a service uses to their `0.1.0` tags together. Mixing `v1.4.0` and per-crate pins for crates
that share types breaks, as described next.

Crates share types: any crate that names a type of a sibling in its public API has the same type as that sibling.
For example `br-core-events` re-exports `UserId`, `ServiceAccountId` and `Actor` from `br-core-kernel`; `br-core-auth`
re-exports `ScopeKey` from `br-core-scope` and names `Actor` from `br-core-kernel`; `br-identity-domain` re-exports
`ScopeDeclaration` from `br-core-scope`; `br-core-integration` re-exports `EventMetadata` from `br-core-events` and,
through it, `Actor`, `UserId` and `ServiceAccountId` from `br-core-kernel`, so `br-core-integration` moves with
`br-core-events` and `br-core-kernel` (`br-core-integration/v0.1.0` and `br-core-events/v0.1.0` with
`br-core-kernel` at `v1.4.0` fail with E0308); `br-util-nats-fabric` re-exports `EventMetadata` from `br-core-events`
and `IntegrationEvent`, `IntegrationCommand`, `MessageOutcome` and the coordinate types (`Bc`, `Aggregate`, `Verb`,
`PastFact`, `CommandCoords`, `EventCoords`, `CoordError`) from `br-core-integration`, so it moves with
`br-core-integration`, `br-core-events` and `br-core-kernel` (`br-util-nats-fabric/v0.1.0` with any of them at `v1.4.0`
fails with E0308 where a coordinate or an envelope crosses); `br-scope-declaration-contract` returns `CommandCoords`,
`EventCoords` and `CoordError` from `br-core-integration`, so it moves with `br-core-integration` (and with
`br-util-nats-fabric` when the same coordinates are handed to the fabric); `br-util-axum-auth` inserts
`br-core-auth`'s `Passport` into the request extensions; `br-util-graphql` wraps `br-core-values`. A consumer must not
mix a per-crate pin and `v1.4.0` for crates that share types: the build then holds two copies of the same type. Move
such crates together, all to per-crate pins or all to `v1.4.0`.

The three crates built on the others name these types, checked against `cargo public-api --all-features`; each fails
with E0308 where a value of one copy crosses to the other:

- `br-util-directory` names `PublishedUser`, `PublishedGroup`, `PublishedServiceAccount` and `DirectoryMeta` (`br-core-directory`); `Fabric`, `FabricError`, `KvKeyError`, `ProjectionError` and `WatchHealthReceiver` (`br-util-nats-fabric`). It also depends on `br-util-postgres` behind its `consumer` feature, with no `br-util-postgres` type in its API.
- `br-util-scope-declaration` names `Fabric` and `FabricError` (`br-util-nats-fabric`); `ScopeDeclaration`, `ServiceKey` and `ServiceScopesRejected` (`br-core-scope`); `ReadinessHandle` (`br-util-axum-readiness`); and returns `Actor`, which is `br-core-kernel`'s type (`br-core-integration` and `br-core-events` re-export it).
- `br-identity-app` names `Fabric` and `FabricError` (`br-util-nats-fabric`); `IntegrationCommand` and `CommandCoords` (`br-core-integration`); `ServiceKey`, `DeclareServiceScopes` and `ScopeDeclarationError` (`br-core-scope`); `ScopeRegistry`, `DeclarationOutcome` and `RegistryHydrationError` (`br-identity-domain`). For example `br-identity-app/v0.1.0` with `br-core-scope` at `v1.4.0` fails to compile.

They move with those crates, and the crates those carry types through (`br-core-integration` re-exports from `br-core-events`, which re-exports from `br-core-kernel`).

Most mixes fail to compile. A type-keyed channel does not: `br-util-axum-auth` inserts `br-core-auth`'s `Passport` into
the request extensions, and a handler extracts it by that type, so `br-util-axum-auth` and the service must use the same
`br-core-auth` source tag. `br-util-axum-auth/v0.1.0` pins `br-core-auth/v0.1.0`: take that pair together. Any other `br-core-auth` tag next to it (`v1.4.0`, or a later `br-core-auth/v0.1.x`), or `br-util-axum-auth` at `v1.4.0` next to `br-core-auth/v0.1.0`, compiles and holds two `Passport` types: every handler that extracts the `Passport` then gets a 500 `Missing request extension` (or `None` for an `Option` extractor). A request without the header still gets 401, and a handler that ignores the `Passport` still gets 200. A service that still mounts `br-util-axum-auth` at `v1.4.0` keeps
`br-core-auth`, `br-core-kernel` and `br-core-scope` on `v1.4.0` until it moves `br-util-axum-auth` to `0.1.0`.

Every pin must name the repository as `https://github.com/BotResources/br-rust-common` (a `.git` suffix or another case
is the same repository): the sibling pins inside the crates hard-code that URL, so a pin through `ssh://git@...` splits
every sibling into two copies. A consumer also needs `allow-git = ["https://github.com/BotResources/br-rust-common"]`
under `[sources]` in its `deny.toml` (without it `cargo-deny` reports `source-not-allowed`) and keeps `version = "x.y.z"`
beside each tag. To turn a silent second copy into a build failure, a consumer bans multiple versions of the crates
whose type appears in the public API of another crate:

```toml
[[bans.deny]]
crate = "br-core-auth"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-directory"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-events"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-integration"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-kernel"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-scope"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-values"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-identity-domain"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-util-axum-readiness"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-util-nats-fabric"
deny-multiple-versions = true
```

Released crates are pinned by their own tag, with the matching `version` beside it (a tag-only git dependency
carries a `*` version requirement, which `cargo-deny` rejects):

```toml
[dependencies]
br-core-kernel = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-kernel", tag = "br-core-kernel/v0.1.0", version = "0.1.0" }
br-core-directory = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-directory", tag = "br-core-directory/v0.1.0", version = "0.1.0" }
br-core-scope = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-scope", tag = "br-core-scope/v0.1.0", version = "0.1.0" }
br-core-values = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-values", tag = "br-core-values/v0.1.0", version = "0.1.0" }
br-util-boot = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-boot", tag = "br-util-boot/v0.1.0", version = "0.1.0" }
br-util-axum-readiness = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-axum-readiness", tag = "br-util-axum-readiness/v0.1.0", version = "0.1.0" }
br-util-broadcast = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-broadcast", tag = "br-util-broadcast/v0.1.0", version = "0.1.0" }
br-util-observability = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-observability", tag = "br-util-observability/v0.1.0", version = "0.1.0" }
br-core-events = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-events", tag = "br-core-events/v0.1.0", version = "0.1.0" }
br-core-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "br-core-auth/v0.1.0", version = "0.1.0" }
br-identity-domain = { git = "https://github.com/BotResources/br-rust-common", package = "br-identity-domain", tag = "br-identity-domain/v0.1.0", version = "0.1.0" }
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres/v0.1.1", version = "0.1.1" }
br-util-graphql = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-graphql", tag = "br-util-graphql/v0.1.0", version = "0.1.0", features = ["graphql"] }
br-core-integration = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-integration", tag = "br-core-integration/v0.1.0", version = "0.1.0" }
br-util-axum-auth = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-axum-auth", tag = "br-util-axum-auth/v0.1.0", version = "0.1.0" }
br-scope-declaration-contract = { git = "https://github.com/BotResources/br-rust-common", package = "br-scope-declaration-contract", tag = "br-scope-declaration-contract/v0.1.0", version = "0.1.0" }
br-util-nats-fabric = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-nats-fabric", tag = "br-util-nats-fabric/v0.1.0", version = "0.1.0", features = ["outbox"] }
br-util-directory = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-directory", tag = "br-util-directory/v0.1.0", version = "0.1.0", features = ["consumer"] }
br-util-scope-declaration = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-scope-declaration", tag = "br-util-scope-declaration/v0.1.0", version = "0.1.0" }
br-identity-app = { git = "https://github.com/BotResources/br-rust-common", package = "br-identity-app", tag = "br-identity-app/v0.1.0", version = "0.1.0" }
```

`br-util-graphql` exports nothing without `features = ["graphql"]`. `br-util-postgres` takes `features = ["migrate"]` for `migrations_status`, `br-util-nats-fabric` takes `features = ["outbox"]` for the Postgres transactional outbox (leave it off otherwise), `br-util-directory` takes `features = ["consumer"]` (the `known_*` projection and readers), `features = ["publisher"]` (the roster writer) or both, and `br-core-auth` takes `features = ["test-support"]` as a dev-dependency only.

The general form is `{ git = "https://github.com/BotResources/br-rust-common", package = "<crate>", tag = "<crate>/v<version>", version = "<version>" }`.

## Release process

1. In your PR, set the crate's `version` in its own `Cargo.toml`, add a plain `## X.Y.Z` section to its `CHANGELOG.md`,
   and update the pin in this README. The version must be a sealed patch of the crate's library in the BotResources
   Libraries registry.
2. CI runs `cargo semver-checks` for each crate against its previous `{crate}/v*` tag and gates the version against
   the registry.
3. On merge to `main`, the `release-tags` workflow tags every tracked crate whose version has no tag yet, with the
   annotated tag `{crate}/v{version}`, creates the GitHub release and records the release in the registry.

## Dev

```bash
cargo build  --workspace
cargo test   --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt    --all
```

MSRV: **1.88** (edition 2024). License: Apache-2.0. Generated by an AI system (Anthropic Claude) under the direction and review of BotResources, which takes full responsibility for it (EU AI Act, Regulation (EU) 2024/1689).
