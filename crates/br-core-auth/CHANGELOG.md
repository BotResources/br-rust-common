# Changelog — br-core-auth

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.5.1] — 2026-05-22

**Changed**
- Workspace metadata cleanup: `edition`, `rust-version`, `license`, and
  `repository` now inherit from `[workspace.package]` via
  `.workspace = true`. The crate's `rust-version` was previously declared as
  `1.85` per-crate while the workspace, CI, and top-level README all
  advertised `1.88`; the inherited value is now consistently `1.88`. No API
  or runtime behavior change.
- README: documented the `bearer_token_key` / `BearerTokenEntry` PAT
  bearer-token contract introduced in 0.5.0. The public API was already
  exported from the crate root in 0.5.0; only the README description was
  catching up.

## [0.5.0] — 2026-05-22

**Added**
- New module exposing the canonical PAT bearer-token contract used across
  services that resolve a PAT into a `Passport`:
  - `bearer_token_key(plaintext: &str) -> String` — lowercase-hex SHA-256
    derivation of the KV-bucket key. Issuance hashes the freshly-generated
    token to write the entry; every authenticated request hashes the inbound
    token to look it up. Both sides MUST go through this function so the
    hashing stays in lockstep.
  - `BearerTokenEntry { email, token_id }` — value stored under that key in
    the `bearer_tokens` NATS KV bucket. `token_id` is the PAT's stable
    identifier (audit / revocation), and is the same UUID surfaced on
    `Passport::Human` via `AuthMethod::Pat { token_id }`.
- Both items are re-exported at the crate root.

**Notes**
- No `Passport` / `AuthMethod` shape change — wire compatibility with 0.4.x
  is preserved.
- The plaintext bearer token is never stored anywhere: only the hash is
  used as the KV key. PATs are server-issued high-entropy random secrets,
  so SHA-256 is the right tool here (key derivation, not password storage).

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
