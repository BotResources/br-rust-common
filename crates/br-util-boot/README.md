# br-util-boot

The boot environment of a BotResources service: the **names** of the variables
it reads at boot, and **`BootEnv`**, one typed, validating reader for them.
Tier `util`; it enforces no domain policy and does no I/O beyond reading the
process environment.

## The code owns the names

A variable a binary reads at boot is part of its ops contract: the Helm chart
that deploys the binary must render it under exactly that name. So the name is
defined **once, in code**, by the crate that reads it — never spelled as a
literal in a service, and never invented by a chart:

| Constant | Name | Read by | Required |
|---|---|---|---|
| `env::ENVIRONMENT` | `ENVIRONMENT` | `BootEnv` → `Environment` | yes |
| `env::PORT` | `PORT` | `BootEnv` → `NonZeroU16` | yes |
| `env::HOST` | `HOST` | `BootEnv` → `IpAddr` | no — `0.0.0.0` (`DEFAULT_HOST`) |
| `env::DATABASE_URL` | `DATABASE_URL` | `BootEnv` → `DatabaseUrl`; also `br_util_postgres::init_migration_pool`'s fallback | yes |
| `env::NATS_URL` | `NATS_URL` | `BootEnv` → `NatsUrl` | yes |

The names a single library reads on its own belong to that library:
`br_util_postgres::env::{DATABASE_URL_OWNER, TRUSTED_NETWORK_HOSTS}`, and the
probe paths `br_util_observability::{LIVENESS_PATH, METRICS_PATH}` and
`br_util_axum_readiness::READINESS_PATH`. The library chart
[`br-common-service`](../../charts/br-common-service/README.md#names) is gated
against all of them in CI: it cannot render a name, or a default path, that
the code does not define.

**Why a crate of its own.** The four names that are not one library's —
`ENVIRONMENT`, `PORT`, `DATABASE_URL`, `NATS_URL` — are read by every service,
and none of the existing crates is the place for them: the observability and
readiness crates serve HTTP routes, `br-util-postgres` owns pools and TLS (and
pulls sqlx), `br-util-nats-fabric` owns the NATS client (and pulls async-nats).
A reader in any of them would drag that crate's dependencies into every binary
that only wants its boot environment, and would make one crate the owner of
another's names. This crate depends on `url` and `thiserror` only, and
`br-util-postgres` depends on it for the one name it shares
(`DATABASE_URL`, its migration-pool fallback).

## What's inside

| Item | Kind | Behavior |
|---|---|---|
| `env` | module of `&str` constants | The names above. |
| `BootEnv` | struct (`#[non_exhaustive]`, public fields) | `environment`, `host`, `port`, `database_url`, `nats_url` — each a type that cannot hold an invalid value. `Debug` is safe to log: both URLs redact their credentials. |
| `BootEnv::from_env` | `fn() -> Result<BootEnv, BootEnvError>` | Reads the process environment. Reads **every** variable and reports **every** problem at once. Never writes the environment. |
| `BootEnv::from_lookup` | `fn(impl FnMut(&str) -> Result<String, VarError>)` | The same, through a lookup shaped like `std::env::var`: a test's map, or the process environment with a `.env` map filling in what it does not set. |
| `BootEnv::listen_addr` | `fn(&self) -> SocketAddr` | `HOST`:`PORT`, for the listener. |
| `Environment` | enum `Local`, `Dev`, `Test`, `Uat`, `Prod` | `FromStr` accepts exactly `local`, `dev`, `test`, `uat`, `prod` — the lowercase spelling of the chart's `botresources.ai/env` label, no other case, no alias. `Display` / `as_str` give it back. Exhaustive on purpose: a new environment makes every `match` (a `Local \| Test`-only gate, say) decide again. |
| `DatabaseUrl` | newtype | A `postgres://` / `postgresql://` URL naming a host. `as_str()` is the value exactly as set, for `br_util_postgres::init_pool`; `redacted()` / `Debug` mask the password (authority or `password` query parameter). No `Display`. Whether the host may be reached without TLS stays `br-util-postgres`'s decision. |
| `NatsUrl` | newtype | A `nats://`, `tls://`, `ws://` or `wss://` URL naming a host — the scheme stated, never implied. `as_str()` for `Fabric::connect`; `redacted()` / `Debug` mask the whole user information (a user and password, or a token). No `Display`. |
| `BootEnvError` | error | Every problem, in reading order (never empty): `problems() -> &[VarProblem]`; `Display` joins them. |
| `VarProblem` | enum (`#[non_exhaustive]`) | `Missing`, `Empty`, `NotUnicode`, `Invalid { reason }` — each names its variable (`var()`). A refusal quotes a plain value (`PORT`, `ENVIRONMENT`, `HOST`) so a typo shows, and never the value of `DATABASE_URL` or `NATS_URL`. |

## Usage

```rust
use axum::Router;
use br_util_axum_readiness::{ReadinessHandle, readiness_router};
use br_util_boot::BootEnv;
use br_util_nats_fabric::Fabric;
use br_util_observability::{init_logging, liveness_router};
use br_util_postgres::{init_migration_pool, init_pool};

init_logging("svc-example");
let boot = BootEnv::from_env()?; // every problem at once, secrets redacted
tracing::info!(?boot, "boot environment");

let migration_pool = init_migration_pool().await?; // DATABASE_URL_OWNER
// … run the migrations, close the pool …
let pool = init_pool(boot.database_url.as_str()).await?;
let fabric = Fabric::connect(boot.nats_url.as_str()).await?;

let readiness = ReadinessHandle::not_ready("starting up");
let app = Router::new()
    .merge(liveness_router())                  // LIVENESS_PATH
    .merge(readiness_router(readiness.clone())); // READINESS_PATH
let listener = tokio::net::TcpListener::bind(boot.listen_addr()).await?;
```

**`.env` files.** `BootEnv` never loads one: `std::env::set_var` is unsound
once threads run, and a `#[tokio::main]` body already has them. A binary that
wants a `.env` for local runs loads it before starting its runtime, or reads it
into a map that fills in what the process environment does not set:

```rust
use std::collections::HashMap;
use br_util_boot::BootEnv;

let file: HashMap<String, String> = HashMap::new(); // e.g. from dotenvy::from_path_iter
let boot = BootEnv::from_lookup(|name| {
    std::env::var(name).or_else(|missing| file.get(name).cloned().ok_or(missing))
})?;
```

## Adoption

Services adopt `BootEnv` in their next sealed patch (see the chart's
[Adoption](../../charts/br-common-service/README.md#adoption)); the names are
the ones they already read, so nothing changes at runtime until then. What a
binary moving from its own reader must know: `ENVIRONMENT` and `PORT` are
required (no default: the chart always renders them), `ENVIRONMENT` is one of
the five lowercase spellings, `HOST` is an IP address, `NATS_URL` states its
scheme.

## Install

```toml
[dependencies]
br-util-boot = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-boot", tag = "v1.4.0", version = "1.4.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
