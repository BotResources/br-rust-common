# Changelog — br-util-postgres

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.6.0] — 2026-06-01

**Added**
- `TRUSTED_NETWORK_HOSTS` environment variable — the canonical name for the
  comma-separated list of DB hosts exempted from the remote-TLS requirement.
  The rename exists to name the real concept: a listed host is exempted
  because it sits on a **trusted network segment**, not because of any
  property of the host itself. BR runs each service alongside its
  CloudNativePG database in the same Kubernetes namespace, behind a
  default-deny `NetworkPolicy`; that intra-namespace app↔DB traffic is
  intentionally plaintext, so there is no untrusted segment between them for
  transport TLS to protect. `TRUSTED_NETWORK_HOSTS` is how a service makes
  that opt-out an explicit, per-host, conscious declaration rather than a
  blanket bypass — the lib stays secure-by-default for genuinely remote
  hosts. Behavior is otherwise identical to the former variable.

**Deprecated**
- `TRUSTED_HOSTS` — the former name. It is still honored as a fallback when
  `TRUSTED_NETWORK_HOSTS` is unset, and `is_on_trusted_network` now emits a
  `tracing::warn!` each time the fallback name is read (it does not warn when
  the new name is set, nor when neither is set). The read happens only at pool
  init, so this fires a couple of times at boot — fine, and not deduplicated.
  No behavior changes for existing deployments; the old name keeps working.
  Removal is targeted for `1.0.0` — rename the variable before then.

## [0.5.3] — 2026-05-22

**Changed**
- Workspace metadata cleanup: `edition`, `rust-version`, `license`, and
  `repository` now inherit from `[workspace.package]` via
  `.workspace = true`. The crate's `rust-version` was previously declared as
  `1.85` per-crate while the workspace, CI, and top-level README all
  advertised `1.88`; the inherited value is now consistently `1.88`. No API
  or runtime behavior change.

## [0.5.2] — 2026-05-17

**Fixed**
- `ensure_app_role` no longer binds the password as a query parameter on the
  `ALTER ROLE` step. Postgres rejects bind parameters in DDL — `ALTER ROLE
  "<name>" PASSWORD $1` fails with `syntax error at or near "$1"`, so 0.5.0
  and 0.5.1 could never actually set or rotate the app password (the
  `permission denied` failure in 0.5.0 masked this; 0.5.1 exposed it). The
  step now embeds the password as a dollar-quoted literal using a per-call
  random tag of the form `br_<uuid-v7-simple>`: `ALTER ROLE "<name>"
  PASSWORD $br_<32hex>$<password>$br_<32hex>$`. Dollar-quoting passes the
  password through byte-for-byte with no escape rules to mishandle, and the
  unguessable tag prevents a malicious password from breaking out of the
  literal. The generated SQL is never logged or surfaced in errors, and the
  string buffer is overwritten with zeros after the query executes to
  shorten the secret's residency in our own memory. Public API is
  unchanged.

## [0.5.1] — 2026-05-17

**Fixed**
- `ensure_app_role` no longer issues the defense-in-depth `ALTER ROLE
  "<name>" LOGIN NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION
  INHERIT` after creation. On PG 16+, that statement requires SUPERUSER even
  when the attributes match the current state, so non-superuser CREATEROLE
  callers (e.g. a CNPG-managed `<svc>_owner`) failed with `permission denied
  to alter role`. `CREATE ROLE ... LOGIN` already defaults to all NO* flags,
  and `ensure_app_role` is the sole creator of `<svc>_app` roles, so the
  hardening step was redundant. The `ALTER ROLE "<name>" PASSWORD $1` step is
  unchanged — it only needs membership in the created role.

## [0.5.0] — 2026-05-16

**Added**
- New `ensure_app_role(pool, role_name, password)` helper for the two-role
  Postgres model. The owner pool calls it at startup, before `sqlx::migrate`,
  to idempotently create the runtime app role with `LOGIN` and set its
  password. The `CREATE ROLE` step runs inside a `DO $$ ... END $$` block
  with an `IF NOT EXISTS` guard; an `ALTER ROLE` step then forces
  `NOSUPERUSER NOCREATEDB NOCREATEROLE NOBYPASSRLS NOREPLICATION INHERIT`
  as defense in depth against out-of-band role creation; the password is
  bound as a query parameter (`ALTER ROLE "<name>" PASSWORD $1`), never
  interpolated.
- New `PostgresError::InvalidRoleName(String)` variant. Role names are
  validated Rust-side against `^[a-z][a-z0-9_]*$` with a 63-byte cap before
  being interpolated into DDL — invalid names are rejected without touching
  the database.

**Changed**
- `grant_app_access` now also runs `ALTER DEFAULT PRIVILEGES IN SCHEMA public
  GRANT … TO <app_role>` for `TABLES` and `SEQUENCES`. Future objects created
  by the owner (typically via later migrations) are automatically GRANTed to
  the app role, closing the gap where a new migration would otherwise create
  a table the app role couldn't access until a redeploy re-ran the bulk
  GRANT.
- `grant_app_access` now validates `app_role` against the same
  `^[a-z][a-z0-9_]*$` rule and double-quotes the identifier in the emitted
  DDL. Pre-0.5 callers passing names with characters outside the regex (or
  longer than 63 bytes) now get `PostgresError::InvalidRoleName` instead of
  a SQL-injected DDL string. No project in this workspace uses such names.

## [0.4.0] — 2026-05-10

**Changed**
- `set_rls_context` now injects two additional Postgres session variables on
  top of the existing three:
  - `app.is_pat` — `"true"` / `"false"` (always `"false"` for Service and JWT).
  - `app.impersonator_id` — admin UUID when impersonating, empty string
    otherwise. Policies test `current_setting('app.impersonator_id') <> ''`.
- Bump of `br-core-auth` dependency to 0.4.

## [0.3.0] — 2026-04-17

- Carved out of `br-service-core` during the workspace split into
  `br-rust-common`. Provides Postgres helpers: `init_pool` /
  `init_migration_pool`, `validate_database_tls`, `set_rls_context`,
  `grant_app_access`.
