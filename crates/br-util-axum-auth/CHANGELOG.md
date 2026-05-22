# Changelog — br-util-axum-auth

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

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
