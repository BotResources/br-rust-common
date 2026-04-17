# br-util-postgres

Postgres helpers for BotResources services: pools, TLS validation, RLS, grants.

**Purpose.** `init_pool` / `init_migration_pool` create connection pools with
TLS validation that mirrors sqlx's `sslmode` parsing. `set_rls_context`
injects `Passport` fields into transaction-local session variables for RLS
policies. `grant_app_access` runs the post-migration GRANTs for the app role.

**When to use.** A service uses sqlx + Postgres + RLS and wants the standard
BotResources wiring (two-pool model, TLS enforcement, transaction-local RLS
via `set_config(..., true)`).

**When not to use.** A service does not use Postgres, or needs a different
pooling/RLS strategy. Don't reach into this crate to "bypass" TLS — set
`ALLOW_INSECURE=true` in non-prod environments instead.

**Current version.** `0.3.0`
