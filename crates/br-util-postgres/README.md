# br-util-postgres

Postgres helpers shared by every BotResources service that uses sqlx:
pools with TLS validation, the two-role app/owner provisioning, and
post-migration grants.

**Purpose.** Standardize the wiring around Postgres so every service makes
the same secure choices: a deliberate TLS posture for remote hosts and a
low-privilege runtime role.

**When to use.** A service uses sqlx + Postgres and wants the BotResources
wiring (two-role model, TLS validation, automatic GRANTs on future tables).

**RLS-context injection is the service's job, not this crate's.** The shape of
the `app.*` session variables an RLS policy reads (which fields, which names) is
project-specific — it depends on the service's Passport claims and its policy
model. Each service injects its own transaction-local context with
`set_config(..., true)`; this crate provides the pool and role wiring underneath,
not the context shape.

**When not to use.** The service does not use Postgres. There is no blanket TLS
bypass: a host reached over plaintext because it sits on a trusted network
segment is declared, per-host, in `TRUSTED_NETWORK_HOSTS` — every other remote
host requires TLS.

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
connect — **unconditionally, with no environment-gated escape hatch**. The only
way to reach a remote host over plaintext is to declare it in
`TRUSTED_NETWORK_HOSTS`. This is defense-in-depth for genuinely remote
databases — a managed/off-cluster Postgres, a cross-segment link. The crate ships a **rustls TLS backend** (`tls-rustls-ring-webpki`: pure-Rust
rustls + the `ring` provider + bundled webpki CA roots, no system trust store or
OpenSSL), so that requirement is fulfillable at runtime.

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
| `init_pool(url) -> PgPool` | Long-lived runtime pool (max 20, min 2 connections). Validates TLS before connecting. **Does not run migrations.** |
| `init_migration_pool() -> PgPool` | Short-lived owner pool (max 2). Reads `DATABASE_URL_OWNER` (falls back to `DATABASE_URL`). Use to run migrations, then drop before creating the app pool. |
| `validate_database_tls(url)` | Standalone TLS validator. `sslmode` is resolved by sqlx itself (single source of truth: `sslmode`/`ssl-mode` alias, case-insensitive, last value wins); the host is judged from the URL **authority** by an independent, fail-closed extractor — deliberately *not* sqlx's, whose absent-host default is `localhost` — and a URL that overrides the target via a `host=`/`hostaddr=` query parameter is rejected outright (the validator cannot vouch for a host it does not judge). Loopback and `TRUSTED_NETWORK_HOSTS` entries (hosts on a trusted network segment, e.g. an intra-namespace CNPG database) are always allowed; every other remote host must carry `sslmode=require/verify-ca/verify-full` — unconditionally, with no escape hatch. Validation only — the bundled rustls backend is what lets such a connection actually complete. |

### Role provisioning

| Item | Role |
|---|---|
| `ensure_app_role(pool, role_name, password)` | Idempotent `CREATE ROLE … LOGIN` (guarded by an `IF NOT EXISTS` `DO` block) + `ALTER ROLE … PASSWORD`. Call at startup via the **owner** pool, before `sqlx::migrate`. Validates `role_name` against `^[a-z][a-z0-9_]*$` (≤63 bytes). The role inherits Postgres's no-privilege defaults from `CREATE ROLE … LOGIN` (NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT) — there is **no** explicit hardening `ALTER`, because on PG 16+ asserting those flags requires SUPERUSER. The password is embedded as a **dollar-quoted literal** with a per-call random UUIDv7 tag, not a bind parameter — Postgres rejects bind params in DDL (`ALTER ROLE … PASSWORD $1` is a syntax error), so dollar-quoting is used instead. The generated SQL is never logged. |
| `grant_app_access(pool, app_role)` | Post-migration GRANTs on schema `public` (USAGE, full CRUD on tables, USAGE+SELECT on sequences) **plus** `ALTER DEFAULT PRIVILEGES` so tables created by future migrations are GRANTed automatically. Must run via the same role that owns subsequent migrations. |

### Migration status (opt-in feature `migrate`)

Answers one question about a live database: **is it exactly at the migration
set embedded in this binary — every migration applied, checksums matching,
nothing dirty and nothing applied that this binary does not carry?** It is the
report a post-deployment probe needs, and the truth travels with the artifact,
so no migration count is ever hardcoded anywhere.

Off by default: the workspace `sqlx` pin does not enable `migrate`, so a
consumer that does not want the helper compiles none of it. Enable the
`migrate` feature (see *Dependency* below) to get it. Building the `Migrator`
with `sqlx::migrate!()` additionally needs sqlx's own `macros` feature in the
consuming crate — this feature only enables `sqlx/migrate`.

```rust
use br_util_postgres::migrations_status;

static MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!();

let status = migrations_status(&pool, &MIGRATOR).await?;
if !status.is_current() {
    eprintln!("{status:?}");
    std::process::exit(1);
}
```

| Item | Role |
|---|---|
| `migrations_status(pool, migrator) -> MigrationsStatus` | Reads `_sqlx_migrations` through sqlx's own `Migrate::list_applied_migrations` + `dirty_version` and diffs it against `migrator.iter()` by version and checksum. **Read-only.** |
| `MigrationsStatus::is_current() -> bool` | The safe default, and the whole gate a probe needs: true when every **embedded** migration is applied with a matching checksum, nothing is dirty, and the database carries no migration this binary lacks — `pending`, `checksum_mismatch` and `applied_not_embedded` all empty and `dirty` `None`. A database ahead of the binary is **not** current: a service that runs `migrate!().run()` at boot crash-loops there on `VersionMissing`. |
| `MigrationsStatus::embedded_applied() -> bool` | The lenient predicate: same, minus the `applied_not_embedded` clause. It answers "is *this binary's* own migration set in place", and is the right gate **only** for a service whose migrator is built with `set_ignore_missing(true)`, which boots over a database ahead of it. |

`MigrationsStatus` is `#[non_exhaustive]`; every version list is sorted ascending:

| Field | Exact meaning |
|---|---|
| `applied: usize` | How many embedded migrations have a row in `_sqlx_migrations`. Presence of the row is the whole criterion: a row whose checksum differs still counts as applied, and so does a row with `success = false` — sqlx's `list_applied_migrations` does not filter on `success`, which is why `dirty` is reported separately. `applied + pending.len()` equals the number of embedded migrations. |
| `pending: Vec<i64>` | Embedded migrations with **no** row in the database — the missing tail. |
| `checksum_mismatch: Vec<i64>` | Embedded migrations whose row exists but whose stored checksum differs from the embedded one — applied-but-since-edited. |
| `applied_not_embedded: Vec<i64>` | Rows in the database with **no** embedded counterpart — the rollback-in-progress signal: the database has been migrated by a newer image than the one now running (a rollback, or a stale replica). They are never counted as pending, they falsify `is_current()` but not `embedded_applied()`, and they name the exact versions to look at when deciding whether to roll forward again or to re-run with `set_ignore_missing(true)`. |
| `dirty: Option<i64>` | The lowest version whose row has `success = false` — a partially applied migration, and the same gate `Migrator::run` applies before doing anything (it aborts with `MigrateError::Dirty`). On Postgres with sqlx 0.8 this is expected to stay `None`: DDL is transactional, so a failed migration rolls its own row back rather than leaving it unsuccessful. It covers the states that outlive that guarantee — a row written by an older tool, a `no_tx` migration, or a hand-repaired table. |

**It never runs, creates, or repairs anything.** No `ensure_migrations_table`,
no `CREATE TABLE`, no `Migrator::run` — this crate does not auto-provision (the
existing `init_pool` contract is the same). A database where
`_sqlx_migrations` does not exist yet is not an error: the missing table
(SQLSTATE `42P01`) is reported as *every embedded migration pending*, and the
table is left absent.

Errors map onto the existing `PostgresError::Db`: a `sqlx::migrate::MigrateError`
is carried as `sqlx::Error::Migrate`. No new error variant.

### Errors

`PostgresError`: `Config(String)`, `InvalidRoleName(String)`,
`Db(#[from] sqlx::Error)`.

## Environment variables

| Variable | Purpose |
|---|---|
| `DATABASE_URL` | App runtime pool URL. |
| `DATABASE_URL_OWNER` | Migration pool URL (falls back to `DATABASE_URL`). |
| `TRUSTED_NETWORK_HOSTS` | Comma-separated hostnames on a trusted network segment, exempted from the remote-TLS requirement. Use to declare a DB host that the service reaches over plaintext because the segment is trusted — e.g. an intra-namespace CloudNativePG database behind a default-deny `NetworkPolicy`. A deliberate, per-host opt-out, not a blanket bypass — and the **only** way to reach a remote host without TLS. The legacy `TRUSTED_HOSTS` name is **no longer read** — use this name only. |

## Two-role startup recipe

```rust
use br_util_postgres::{
    ensure_app_role, grant_app_access, init_pool, init_migration_pool,
};

// 1. Owner pool — provisions the runtime role and runs migrations.
let owner = init_migration_pool().await?;
ensure_app_role(&owner, "myservice_app", &app_password).await?;
sqlx::migrate!().run(&owner).await?;
grant_app_access(&owner, "myservice_app").await?;
drop(owner);

// 2. App pool — used for the rest of the process lifetime.
let pool = init_pool(&app_database_url).await?;

// 3. Per-request: open a transaction, inject the service's own RLS context
//    via set_config(..., true), query, commit.
let mut tx = pool.begin().await?;
sqlx::query("SELECT set_config('app.current_user_id', $1, true)")
    .bind(actor_id.to_string())
    .execute(&mut *tx)
    .await?;
let rows = sqlx::query("SELECT id FROM orders").fetch_all(&mut *tx).await?;
tx.commit().await?;
```

### Wiring readiness (fail loud if the DB is unreachable)

`init_pool` returning `Ok` does **not** prove the database is reachable: sqlx
fills `min_connections` lazily, so the failure surfaces on the first query, not
at init. To actually realize the fail-loud invariant, probe once after init and
only then mark the service ready (with [`br-util-axum-readiness`](../br-util-axum-readiness)):

```rust
use br_util_axum_readiness::ReadinessHandle;

let readiness = ReadinessHandle::not_ready("connecting to database");
let pool = init_pool(&app_database_url).await?;
// Force a real connection — this is what `Ok` from `init_pool` did NOT do.
sqlx::query("SELECT 1").execute(&pool).await?; // error here ⇒ stay not-ready
readiness.set_ready();
```

Add to `Cargo.toml`:

```toml
[dependencies]
br-util-postgres = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "v1.3.0", version = "1.3.0" }
# with the migration-status helper:
# br-util-postgres = { git = "...", package = "br-util-postgres", tag = "v1.3.0", version = "1.3.0", features = ["migrate"] }
```

## sqlx is part of the public contract

This crate's public API exposes sqlx 0.8 types directly: `init_pool` returns a
`PgPool` and `PostgresError::Db` wraps `sqlx::Error`. A sqlx **major** bump is
therefore a **breaking release of this crate** and a coordinated migration
across consumers — never a silent
dependency bump. Let this crate's pin drive your sqlx version rather than
pinning sqlx independently.

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
