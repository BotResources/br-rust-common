# br-rust-common

Small, reusable Rust crates for [BotResources](https://botresources.ai) services.

## Catalog

| Crate | Tier | Description | Docs | Changelog |
|---|---|---|---|---|
| `br-core-kernel` | core | Typed ID wrappers (`UserId`, `ServiceAccountId`) | [README](crates/br-core-kernel/README.md) | [CHANGELOG](crates/br-core-kernel/CHANGELOG.md) |
| `br-core-auth` | core | `Passport` DTO + `X-Passport` header codec | [README](crates/br-core-auth/README.md) | [CHANGELOG](crates/br-core-auth/CHANGELOG.md) |
| `br-core-events` | core | Shared event envelopes (`EventMetadata`, `RawEvent`, `DomainEvent`) | [README](crates/br-core-events/README.md) | [CHANGELOG](crates/br-core-events/CHANGELOG.md) |
| `br-util-postgres` | util | Postgres pools, TLS, RLS context, app role, GRANTs | [README](crates/br-util-postgres/README.md) | [CHANGELOG](crates/br-util-postgres/CHANGELOG.md) |
| `br-util-axum-auth` | util | Axum middleware that injects `Passport` from `X-Passport` | [README](crates/br-util-axum-auth/README.md) | [CHANGELOG](crates/br-util-axum-auth/CHANGELOG.md) |

## Architecture

- `core` — cross-cutting constraints, **no dependency on `util`**.
- `util` — optional technical wrappers; may depend on `core`.
- No `svc-*` or business logic in this repo. Each crate defines its own errors.

## Distribution

Not published on crates.io. Each crate is versioned and tagged independently
(`<crate-name>-vX.Y.Z`) and consumed by git tag:

```toml
[dependencies]
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres-v0.5.0" }
```

## Dev

```bash
cargo build  --workspace
cargo test   --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt    --all
```

MSRV: **1.85** (edition 2024). License: MIT.
