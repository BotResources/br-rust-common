# Changelog — br-util-postgres

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.5.0] — Unreleased

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
