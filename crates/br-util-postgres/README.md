# br-util-postgres

Postgres helpers shared by every BotResources service that uses sqlx:
pools with TLS validation, the two-role app/owner provisioning, RLS
context injection, and post-migration grants.

**Purpose.** Standardize the wiring around Postgres so every service makes
the same secure choices: a deliberate TLS posture for remote hosts, a
low-privilege runtime role enforced by RLS, transaction-local identity
injection that can't leak across pooled connections.

**When to use.** A service uses sqlx + Postgres + RLS and wants the
BotResources wiring (two-role model, TLS validation, transaction-local RLS
via `set_config(..., true)`, automatic GRANTs on future tables).

**When not to use.** The service does not use Postgres, or needs a
fundamentally different RLS strategy. Don't reach into this crate to
"bypass" TLS — set `ALLOW_INSECURE=true` in non-prod environments instead.

## The deployment model, and what TLS actually buys here

Read this before you reason about TLS — the default mental model ("encrypt
the app→DB link") does **not** match how these services are deployed.

The typical deployment is Kubernetes (K3s) with a **default-deny
`NetworkPolicy` per namespace** (Kyverno-generated) and **CloudNativePG
(CNPG)** running the Postgres cluster **in the service's own namespace**.
App↔DB traffic is therefore intra-namespace, pod-to-pod, on a network
segment that the default-deny policy already isolates from everything else.
The DB host is a CNPG service name — non-loopback, "remote-looking" — but it
sits on that **trusted, isolated segment**, and TLS to it is **deliberately
not used**: there is no untrusted hop between the app and its database for
transport encryption to protect.

So a service running this way **declares that host** via
`TRUSTED_NETWORK_HOSTS`. That is a per-host, conscious opt-out — we are
saying we trust the *network segment*, not that we have verified transport
encryption. It is never a blanket bypass: only the hosts you name are
exempt, and the crate stays secure-by-default for every other host.

**The default for anything else.** Any non-loopback host that is **not**
declared trusted must carry `sslmode=require` (or `verify-ca` /
`verify-full`) in its URL, or `init_pool` / `init_migration_pool` refuse to
connect (unconditionally in `Prod`; overridable by `ALLOW_INSECURE` only in
non-prod). This is defense-in-depth for genuinely remote databases — a
managed/off-cluster Postgres, a cross-segment link. As of 0.6.1 the crate
ships a **rustls TLS backend** (`tls-rustls-ring-webpki`: pure-Rust rustls +
the `ring` provider + bundled webpki CA roots, no system trust store or
OpenSSL), so that requirement is actually fulfillable at runtime. Before
0.6.1 the workspace built sqlx without any TLS backend, so a `sslmode=require`
URL would fail to connect with "TLS upgrade required … built without TLS
support enabled" — the requirement was validated but unbacked.

### `TRUSTED_NETWORK_HOSTS` matching contract

The match is intentionally literal. An entry exempts a host **only** when it
equals the host extracted from the URL, exactly:

- **Bare hostnames, exact string match.** `cnpg-rw` matches host `cnpg-rw`
  and nothing else. No suffix/subdomain matching, no wildcards.
- **Case-sensitive.** `CNPG-RW` does not match `cnpg-rw`.
- **Port-independent, and an entry must not include a port.** The matcher
  compares against the *host* only (the port is stripped during URL
  parsing). An entry that contains `:port` (e.g. `cnpg-rw:5432`) therefore
  matches **no** host — list the bare hostname.
- **Parsing fails closed.** A URL whose host can't be parsed extracts to the
  empty string, which is on no trusted list, so TLS is required rather than
  skipped. Empty / whitespace-only list entries are dropped, so the trusted
  list can never contain `""`.
- Loopback (`localhost`, `127.0.0.1`, `::1`) is always trusted regardless of
  the list, and short-circuits before the list (and its env read) is touched.

## What's inside

### Connection pools & TLS

| Item | Role |
|---|---|
| `init_pool(url, env, allow_insecure) -> PgPool` | Long-lived runtime pool (max 20, min 2 connections). Validates TLS before connecting. **Does not run migrations.** |
| `init_migration_pool(env, allow_insecure) -> PgPool` | Short-lived owner pool (max 2). Reads `DATABASE_URL_OWNER` (falls back to `DATABASE_URL`). Use to run migrations, then drop before creating the app pool. |
| `validate_database_tls(url, env, allow_insecure)` | Standalone TLS validator: mirrors sqlx's `sslmode` parsing (accepts `sslmode` and `ssl-mode`, case-insensitive, last value wins). Loopback and `TRUSTED_NETWORK_HOSTS` entries (hosts on a trusted network segment, e.g. an intra-namespace CNPG database) are always allowed; every other remote host must carry `sslmode=require/verify-ca/verify-full` unless `allow_insecure` is set in non-prod. Validation only — the rustls backend (since 0.6.1) is what lets such a connection actually complete. |
| `Environment` | Enum: `Local`, `Dev`, `Test`, `Prod`. Only `Prod` is load-bearing today (forbids the `allow_insecure` bypass). |

### Role provisioning

| Item | Role |
|---|---|
| `ensure_app_role(pool, role_name, password)` | Idempotent `CREATE ROLE … LOGIN` (guarded by an `IF NOT EXISTS` `DO` block) + `ALTER ROLE … PASSWORD`. Call at startup via the **owner** pool, before `sqlx::migrate`. Validates `role_name` against `^[a-z][a-z0-9_]*$` (≤63 bytes). The role inherits Postgres's no-privilege defaults from `CREATE ROLE … LOGIN` (NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT) — there is **no** explicit hardening `ALTER`, because on PG 16+ asserting those flags requires SUPERUSER (dropped in 0.5.1). The password is embedded as a **dollar-quoted literal** with a per-call random UUIDv7 tag, not a bind parameter — Postgres rejects bind params in DDL (`ALTER ROLE … PASSWORD $1` is a syntax error), so the 0.5.2 fix dollar-quotes instead. The generated SQL is never logged. |
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
| `TRUSTED_NETWORK_HOSTS` | Comma-separated hostnames on a trusted network segment, exempted from the remote-TLS requirement. Use to declare a DB host that the service reaches over plaintext because the segment is trusted — e.g. an intra-namespace CloudNativePG database behind a default-deny `NetworkPolicy`. A deliberate, per-host opt-out, not a blanket bypass. |
| `TRUSTED_HOSTS` | **Deprecated** (since 0.6.0; removal targeted for 1.0.0). Former name of `TRUSTED_NETWORK_HOSTS`; still honored as a fallback when the new name is unset, and warns on use. Rename it. |

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
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres-v0.6.1" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
