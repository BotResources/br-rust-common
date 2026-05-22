# br-util-postgres

Postgres helpers shared by every BotResources service that uses sqlx:
pools with TLS validation, the two-role app/owner provisioning, RLS
context injection, and post-migration grants.

**Purpose.** Standardize the wiring around Postgres so every service makes
the same secure choices: TLS-on-by-default for remote hosts, a low-privilege
runtime role enforced by RLS, transaction-local identity injection that
can't leak across pooled connections.

**When to use.** A service uses sqlx + Postgres + RLS and wants the
BotResources wiring (two-role model, TLS enforcement, transaction-local RLS
via `set_config(..., true)`, automatic GRANTs on future tables).

**When not to use.** The service does not use Postgres, or needs a
fundamentally different RLS strategy. Don't reach into this crate to
"bypass" TLS — set `ALLOW_INSECURE=true` in non-prod environments instead.

## What's inside

### Connection pools & TLS

| Item | Role |
|---|---|
| `init_pool(url, env, allow_insecure) -> PgPool` | Long-lived runtime pool (max 20, min 2 connections). Validates TLS before connecting. **Does not run migrations.** |
| `init_migration_pool(env, allow_insecure) -> PgPool` | Short-lived owner pool (max 2). Reads `DATABASE_URL_OWNER` (falls back to `DATABASE_URL`). Use to run migrations, then drop before creating the app pool. |
| `validate_database_tls(url, env, allow_insecure)` | Standalone TLS validator: mirrors sqlx's `sslmode` parsing (accepts `sslmode` and `ssl-mode`, case-insensitive, last value wins). Localhost and `TRUSTED_HOSTS` entries are always allowed; remote hosts must have `sslmode=require/verify-ca/verify-full` unless `allow_insecure` is set in non-prod. |
| `Environment` | Enum: `Local`, `Dev`, `Test`, `Prod`. Only `Prod` is load-bearing today (forbids the `allow_insecure` bypass). |

### Role provisioning

| Item | Role |
|---|---|
| `ensure_app_role(pool, role_name, password)` | Idempotent CREATE ROLE + harden + ALTER PASSWORD. Call at startup via the **owner** pool, before `sqlx::migrate`. Validates `role_name` against `^[a-z][a-z0-9_]*$` (≤63 bytes); forces `NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT`; binds the password as a parameter (never interpolated). |
| `grant_app_access(pool, app_role)` | Post-migration GRANTs on schema `public` (USAGE, full CRUD on tables, USAGE+SELECT on sequences) **plus** `ALTER DEFAULT PRIVILEGES` so tables created by future migrations are GRANTed automatically. Must run via the same role that owns subsequent migrations. |

### RLS

| Item | Role |
|---|---|
| `set_rls_context(tx, passport)` | Inside an explicit transaction, injects five `app.*` session variables via `set_config(..., true)` (transaction-local). Variables: `current_user_id`, `is_super_admin`, `is_active`, `is_pat`, `impersonator_id`. **Requires a transaction**; outside one the values are discarded immediately. |

### Errors

`PostgresError`: `Config(String)`, `InvalidRoleName(String)`,
`Db(#[from] sqlx::Error)`.

## Environment variables

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | App runtime pool URL. |
| `DATABASE_URL_OWNER` | Migration pool URL (falls back to `DATABASE_URL`). |
| `ALLOW_INSECURE` | When `true`, lets non-prod environments connect over plaintext. Ignored in `Prod`. |
| `TRUSTED_HOSTS` | Comma-separated hostnames treated as local (no TLS required). Use for Docker Compose service names (`postgres`, `nats`). |

## Two-role startup recipe

```rust
use br_util_postgres::{
    ensure_app_role, grant_app_access, init_pool, init_migration_pool,
    Environment, set_rls_context,
};

// 1. Owner pool — provisions the runtime role and runs migrations.
let owner = init_migration_pool(Environment::Prod, false).await?;
ensure_app_role(&owner, "myservice_app", &app_password).await?;
sqlx::migrate!().run(&owner).await?;
grant_app_access(&owner, "myservice_app").await?;
drop(owner);

// 2. App pool — used for the rest of the process lifetime.
let pool = init_pool(&app_database_url, Environment::Prod, false).await?;

// 3. Per-request: open a transaction, inject identity, query.
let mut tx = pool.begin().await?;
set_rls_context(&mut tx, &passport).await?;
let rows = sqlx::query("SELECT id FROM orders").fetch_all(&mut *tx).await?;
tx.commit().await?;
```

Add to `Cargo.toml`:

```toml
[dependencies]
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres-v0.5.3" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
