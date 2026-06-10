# Changelog — br-util-axum-auth

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.4.2] — 2026-06-10

**Changed (Security)**
- `passport_header_middleware` now returns a **uniform, opaque 401** (constant
  body `"unauthorized"`) for every rejection cause. Previously the body
  disclosed which check failed (`"missing X-Passport header"` /
  `"missing or empty X-Passport"` / `"malformed X-Passport header"`), a small
  validation oracle for an unauthenticated caller; the empty/non-UTF8 case also
  reported an inaccurate message. The precise cause now goes to
  `tracing::warn!` server-side (one event per rejection, indicating which check
  failed); the header **value** is never logged, as it may carry a forged
  passport payload. These bodies are not consumed by any frontend (the gateway
  forwards only status codes), so the change is caller-invisible beyond the
  body text.
- Bump of `br-core-auth` dependency to 0.6 (strict `Passport`
  deserialization). No own API change.

## [0.4.1] — 2026-05-22

**Changed**
- Workspace metadata cleanup: `edition`, `rust-version`, `license`, and
  `repository` now inherit from `[workspace.package]` via
  `.workspace = true`. The crate's `rust-version` was previously declared as
  `1.85` per-crate while the workspace, CI, and top-level README all
  advertised `1.88`; the inherited value is now consistently `1.88`. No API
  or runtime behavior change.

## [0.4.0] — 2026-05-10

**Changed**
- Bump of `br-core-auth` dependency to 0.4. No own API change.

## [0.3.0] — 2026-04-17

- Carved out of `br-service-core` during the workspace split into
  `br-rust-common`. Axum middleware that injects `Passport` from the
  `X-Passport` header.
