# br-rust-common

> [!IMPORTANT]
> **This repository is maintained for BotResources and its authorized clients.**
> It is published under MIT and made available read-only for visibility and
> dependency consumption. The MIT license governs your rights to use, modify,
> and fork the code; the rest of this notice describes our operational stance,
> not a legal restriction.
>
> **We do not accept external pull requests, issues, or support requests.**
> Issues and Discussions are disabled. PRs from accounts that are not on the
> internal contributor allowlist will be closed without review. Forks are
> permitted by MIT and we do not (and cannot) prevent them; we simply do not
> monitor, support, or accept contributions from forks outside the BR
> commercial relationship.
>
> - Clients with a commercial relationship: contact your BR account manager.
> - Security reports: see [SECURITY.md](SECURITY.md) (private email channel).
> - This is not a community-supported project. No support is provided through
>   GitHub.

Small, reusable Rust crates for [BotResources](https://botresources.ai) services.

## Catalog

| Crate | Tier | Description | Docs | Changelog |
|---|---|---|---|---|
| `br-core-kernel` | core | Typed ID wrappers (`UserId`, `ServiceAccountId`) | [README](crates/br-core-kernel/README.md) | [CHANGELOG](crates/br-core-kernel/CHANGELOG.md) |
| `br-core-auth` | core | `Passport` DTO, `X-Passport` header codec, PAT bearer-token contract | [README](crates/br-core-auth/README.md) | [CHANGELOG](crates/br-core-auth/CHANGELOG.md) |
| `br-core-events` | core | Shared event envelopes (`EventMetadata`, `RawEvent`, `DomainEvent`) | [README](crates/br-core-events/README.md) | [CHANGELOG](crates/br-core-events/CHANGELOG.md) |
| `br-core-integration` | core | Typed integration envelopes + `IntegrationPublisher` (NATS JetStream / noop) | [README](crates/br-core-integration/README.md) | [CHANGELOG](crates/br-core-integration/CHANGELOG.md) |
| `br-core-scope` | core | Scope self-declaration contract types (`ScopeKey`, `ScopeDeclaration`, declare/accepted/rejected payloads) | [README](crates/br-core-scope/README.md) | [CHANGELOG](crates/br-core-scope/CHANGELOG.md) |
| `br-core-values` | core | Universal value objects: `Localized<F, L>` text family + ISO `Money` / `Currency` / `CountryCode` | [README](crates/br-core-values/README.md) | [CHANGELOG](crates/br-core-values/CHANGELOG.md) |
| `br-util-postgres` | util | Postgres pools, TLS, RLS context, app role, GRANTs | [README](crates/br-util-postgres/README.md) | [CHANGELOG](crates/br-util-postgres/CHANGELOG.md) |
| `br-util-axum-auth` | util | Axum middleware that injects `Passport` from `X-Passport` | [README](crates/br-util-axum-auth/README.md) | [CHANGELOG](crates/br-util-axum-auth/CHANGELOG.md) |
| `br-util-axum-readiness` | util | Readiness gate (`/readyz`) for HTTP services | [README](crates/br-util-axum-readiness/README.md) | [CHANGELOG](crates/br-util-axum-readiness/CHANGELOG.md) |
| `br-util-observability` | util | Boot-time observability: structured JSON logging + an always-200 `/livez` liveness route | [README](crates/br-util-observability/README.md) | [CHANGELOG](crates/br-util-observability/CHANGELOG.md) |
| `br-util-scope-declaration` | util | Boot-time scope-declaration handshake helper (declare scopes to Identity, gate readiness on the confirmation) | [README](crates/br-util-scope-declaration/README.md) | [CHANGELOG](crates/br-util-scope-declaration/CHANGELOG.md) |
| `br-identity-domain` | bc | Identity bounded context, pure domain — scope-registration slice (`ScopeRegistry` aggregate, commands, events) | [README](crates/br-identity-domain/README.md) | [CHANGELOG](crates/br-identity-domain/CHANGELOG.md) |
| `br-identity-app` | bc | Identity bounded context, application/adapter half — scope-registration slice (Postgres persistence, durable NATS consumer, `load → judge → save → dispatch` pipeline, confirmations) | [README](crates/br-identity-app/README.md) | [CHANGELOG](crates/br-identity-app/CHANGELOG.md) |

## Architecture

- `core` — cross-cutting constraints, **no dependency on `util`**.
- `util` — optional technical wrappers; may depend on `core`.
- `bc` — a packaged bounded context (`*-domain` + `*-app`), reusable per project; builds on `core` / `util`.
- No `svc-*` or business logic in this repo. Each crate defines its own errors.

## Distribution

Not published on crates.io. Each crate is versioned and tagged independently
(`<crate-name>-vX.Y.Z`) and consumed by git tag:

```toml
[dependencies]
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres-v0.7.0" }
```

## Release process

1. In your PR, bump the affected crate's `Cargo.toml` version and add a
   matching `## [X.Y.Z] — YYYY-MM-DD` section to its `CHANGELOG.md`.
2. Open the PR. CI runs `cargo semver-checks` per crate against its own
   latest tag (`<crate>-v…`), so a version bump that doesn't match the
   actual API change will fail the check.
3. On merge to `main`, the `release-tags` workflow scans every
   `crates/*/Cargo.toml`, creates the matching `<crate>-vX.Y.Z` annotated
   tag if missing, and pushes it. That tag *is* the published version —
   downstream consumers pin to it.

## Dev

```bash
cargo build  --workspace
cargo test   --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt    --all
```

MSRV: **1.88** (edition 2024). License: MIT.
