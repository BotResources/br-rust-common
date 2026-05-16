# Changelog — br-core-auth

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.4.0] — 2026-05-10

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

## [0.3.0] — 2026-04-17

- Carved out of `br-service-core` during the workspace split into
  `br-rust-common`. Provides the `Passport` DTO + `PassportHeader` trait for
  the `X-Passport` header.
