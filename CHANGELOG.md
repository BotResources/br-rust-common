# Changelog

All notable changes to the crates in this workspace are documented in this
file. Format inspired by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
crates follow [SemVer](https://semver.org/) and are versioned independently.

## [0.4.0] — Unreleased

### `br-core-auth` — 0.4.0 (breaking)

**Added**
- New public type `AuthMethod` (`Jwt` | `Pat { token_id: Uuid }`), re-exported
  from the crate root, with helpers `is_pat()`, `pat_token_id()`.
- `Passport::Human` gains two required typed fields:
  - `auth_method: AuthMethod` — distinguishes JWT vs PAT credentials, with the
    PAT's `token_id` carried inline for audit / revocation.
  - `impersonator: Option<Uuid>` — `Some(admin_id)` when an admin acts on
    behalf of `user_id`; `None` for a direct request.
- New `Passport` helpers: `auth_method()`, `is_pat()`, `is_impersonating()`,
  `impersonator_id()`.

**Breaking**
- Constructors of `Passport::Human { .. }` must now supply `auth_method` and
  `impersonator`. Pattern-matches using `Passport::Human { .., .. }` (the `..`
  rest pattern) are unaffected.
- JSON wire format gains an `auth_method` object on the `human` variant
  (`{"method": "jwt"}` or `{"method": "pat", "token_id": "..."}`). Producers
  emitting the old shape will be rejected at deserialization.
- `impersonator` is `#[serde(default)]` → safely absent on the wire for
  non-impersonated requests.

**Migration**
- Producers (per-project `svc-identity`): populate `auth_method` at Passport
  construction. Set `impersonator` to `Some(admin_id)` on impersonated
  requests; leave `None` otherwise. The effective `user_id` stays the
  impersonated user so RLS continues to apply their permissions naturally —
  `impersonator` is the audit trail.

### `br-util-postgres` — 0.4.0

**Changed**
- `set_rls_context` now injects two additional Postgres session variables on
  top of the existing three:
  - `app.is_pat` — `"true"` / `"false"` (always `"false"` for Service and JWT).
  - `app.impersonator_id` — admin UUID when impersonating, empty string
    otherwise. Policies test `current_setting('app.impersonator_id') <> ''`.
- Bump of `br-core-auth` dependency to 0.4.

### `br-util-axum-auth` — 0.4.0

**Changed**
- Bump of `br-core-auth` dependency to 0.4. No own API change.

### `br-core-kernel` — unchanged (0.3.0)
### `br-core-events` — unchanged (0.3.0)

## [0.3.0] — 2026-05-10

- Workspace split: `br-service-core` carved into independent crates
  (`br-core-auth`, `br-core-events`, `br-core-kernel`, `br-util-axum-auth`,
  `br-util-postgres`) and the repo renamed to `br-rust-common`.

## [0.2.0] — earlier

- `set_rls_context` switched to transaction-local `set_config(..., true)` to
  eliminate RLS identity leakage on pooled connections.
