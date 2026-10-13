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
| `br-core-integration` | core | Transport-independent integration contracts: message coordinates, `IntegrationEvent`/`IntegrationCommand` envelopes, `MessageOutcome`, and the pure outbox state machine (no NATS, no DB) | `v1.4.0` only | [CHANGELOG](crates/br-core-integration/CHANGELOG.md) |
| `br-core-scope` | core | Scope self-declaration contract types (`ScopeKey`, `ScopeDeclaration`, declare/accepted/rejected payloads) | `0.1.0` | [CHANGELOG](crates/br-core-scope/CHANGELOG.md) |
| `br-core-values` | core | Universal value objects: `Localized<F, L>` text family + ISO `Money` / `Currency` / `CountryCode` | `0.1.0` | [CHANGELOG](crates/br-core-values/CHANGELOG.md) |
| `br-core-directory` | core | Frozen read contract for the identity Published Language: `PublishedUser` / `PublishedGroup` directory entries, the `identity/_meta` manifest and the KV key conventions | `0.1.0` | [CHANGELOG](crates/br-core-directory/CHANGELOG.md) |
| `br-scope-declaration-contract` | core | Single source of the identity service-scope declaration wire coordinates (bc/aggregate/version/command + subject helpers), shared by `br-identity-app` and `br-util-scope-declaration` | `v1.4.0` only | [CHANGELOG](crates/br-scope-declaration-contract/CHANGELOG.md) |
| `br-util-boot` | util | The boot environment of a service: the names it reads at boot (`ENVIRONMENT`, `PORT`, `HOST`, `DATABASE_URL`, `NATS_URL`) and `BootEnv`, one typed, validating reader that reports every problem at once and never logs a credential | `0.1.0` | [CHANGELOG](crates/br-util-boot/CHANGELOG.md) |
| `br-util-postgres` | util | Postgres pools, TLS, app role, GRANTs; owns the names `DATABASE_URL_OWNER`, `TRUSTED_NETWORK_HOSTS` | `0.1.0` | [CHANGELOG](crates/br-util-postgres/CHANGELOG.md) |
| `br-util-axum-auth` | util | Axum middleware that injects `Passport` from `X-Passport` | `v1.4.0` only | [CHANGELOG](crates/br-util-axum-auth/CHANGELOG.md) |
| `br-util-axum-readiness` | util | Readiness gate for HTTP services, served on `READINESS_PATH` (`/readyz`) | `0.1.0` | [CHANGELOG](crates/br-util-axum-readiness/CHANGELOG.md) |
| `br-util-broadcast` | util | In-process event bus (tokio broadcast) for post-commit fan-out of domain events to same-process GraphQL subscriptions; the API shape forbids publishing before the tx commits | `0.1.0` | [CHANGELOG](crates/br-util-broadcast/CHANGELOG.md) |
| `br-util-graphql` | util | GraphQL/REST edge kit: `ErrorCode` cross-service contract, `Affordance` / `MutationResult` / `Connection` / `SubscriptionPayload`, fallible `br-core-values` wrappers | `0.1.0` | [CHANGELOG](crates/br-util-graphql/CHANGELOG.md) |
| `br-util-nats-fabric` | util | The Project NATS Fabric API: coordinate-only publish/consume/await over the fixed `integration.…` subject grammar, the KV published-language projection, and the Postgres transactional outbox (store + subscribe-driven relay) | `v1.4.0` only | [CHANGELOG](crates/br-util-nats-fabric/CHANGELOG.md) |
| `br-util-directory` | util | Publisher and consumer kit for the identity Published Language over the NATS Fabric: reconcile and orphan-delete (publisher), KV to Postgres projector over the `known_*` tables (consumer) | `v1.4.0` only | [CHANGELOG](crates/br-util-directory/CHANGELOG.md) |
| `br-util-observability` | util | Boot-time observability: structured JSON logging, an always-200 liveness route on `LIVENESS_PATH` (`/livez`), the Prometheus exposition on `METRICS_PATH` (`/metrics`) | `0.1.0` | [CHANGELOG](crates/br-util-observability/CHANGELOG.md) |
| `br-util-scope-declaration` | util | Boot-time scope-declaration handshake helper (declare scopes to Identity, gate readiness on the confirmation) | `v1.4.0` only | [CHANGELOG](crates/br-util-scope-declaration/CHANGELOG.md) |
| `br-identity-domain` | bc | Identity bounded context, pure domain — scope-registration slice (`ScopeRegistry` aggregate, commands, events) | `0.1.0` | [CHANGELOG](crates/br-identity-domain/CHANGELOG.md) |
| `br-identity-app` | bc | Identity bounded context, application/adapter half — scope-registration slice (Postgres persistence, durable NATS consumer, `load → judge → save → dispatch` pipeline, confirmations) | `v1.4.0` only | [CHANGELOG](crates/br-identity-app/CHANGELOG.md) |
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
is never released). Crates are released in waves, a crate only after the sibling crates it depends on.

Release rule, enforced by `check-sibling-pins.sh`: a released crate pins each internal sibling it depends on by tag,
as `{ git = "https://github.com/BotResources/br-rust-common", tag = "<sibling>/v<version>", version = "<version>" }`,
with a root `[patch."https://github.com/BotResources/br-rust-common"]` to the local path of the sibling. A crate stays
at `0.0.0` until the siblings it depends on are tagged.

A crate marked `v1.4.0` only in the catalog is not yet released on its own line (it is at `0.0.0` in this tree).
It has shipped in every `v1.x` workspace release: until its wave, the only pin for it is the workspace tag
`v1.4.0`, the last release of the whole workspace under one version (tag stays, never rewritten):

```toml
br-util-nats-fabric = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-nats-fabric", tag = "v1.4.0", version = "1.4.0" }
```

Crates share types: any crate that names a type of a sibling in its public API has the same type as that sibling.
For example `br-core-events` re-exports `UserId`, `ServiceAccountId` and `Actor` from `br-core-kernel`; `br-core-auth`
re-exports `ScopeKey` from `br-core-scope` and names `Actor` from `br-core-kernel`; `br-identity-domain` re-exports
`ScopeDeclaration` from `br-core-scope`; `br-core-integration` re-exports from `br-core-events`; `br-util-graphql` wraps
`br-core-values`; `br-util-directory` returns `PublishedUser`, `PublishedGroup`, `PublishedServiceAccount` and
`DirectoryMeta` from `br-core-directory`; `br-util-scope-declaration` takes `ScopeDeclaration` from `br-core-scope` and
`ReadinessHandle` from `br-util-axum-readiness`. A consumer must not mix a per-crate pin and `v1.4.0` for crates that
share types: the build then holds two copies of the same type. Move such crates together, all to per-crate pins or all
to `v1.4.0`.

Most mixes fail to compile. A type-keyed channel does not: `br-util-axum-auth` (`v1.4.0` only) inserts `br-core-auth`'s
`Passport` into the request extensions, so with `br-core-auth/v0.1.0` it compiles and every handler gets a 500 or no
passport. Keep `br-core-auth`, `br-core-kernel` and `br-core-scope` on `v1.4.0` in any service that mounts
`br-util-axum-auth` until `br-util-axum-auth` is released.

Every pin must name the repository as `https://github.com/BotResources/br-rust-common` (a `.git` suffix or another case
is the same repository): the sibling pins inside the crates hard-code that URL, so a pin through `ssh://git@...` splits
every sibling into two copies. A consumer also needs `allow-git = ["https://github.com/BotResources/br-rust-common"]`
under `[sources]` in its `deny.toml` (without it `cargo-deny` reports `source-not-allowed`) and keeps `version = "x.y.z"`
beside each tag. To turn a silent second copy into a build failure, a consumer bans multiple versions of the shared
crates:

```toml
[[bans.deny]]
crate = "br-core-auth"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-kernel"
deny-multiple-versions = true

[[bans.deny]]
crate = "br-core-scope"
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
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres/v0.1.0", version = "0.1.0" }
br-util-graphql = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-graphql", tag = "br-util-graphql/v0.1.0", version = "0.1.0", features = ["graphql"] }
```

`br-util-graphql` exports nothing without `features = ["graphql"]`. `br-util-postgres` takes `features = ["migrate"]` for `migrations_status`, and `br-core-auth` takes `features = ["test-support"]` as a dev-dependency only.

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
