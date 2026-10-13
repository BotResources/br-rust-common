# Changelog

All notable changes to `br-util-axum-auth` are documented here. The git tag
`br-util-axum-auth/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-util-axum-auth` as its own library, tagged `br-util-axum-auth/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line. No source file differs from `v1.4.0`.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-axum-auth", tag = "br-util-axum-auth/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pin: this crate depends on `br-core-auth`, pinned by tag `br-core-auth/v0.1.0`, and inserts its `Passport` into the request extensions, so a consumer resolves one copy of the type they share. Take `br-util-axum-auth` and `br-core-auth` at `0.1.0` together: the request extension is keyed by the `Passport` type, so `br-util-axum-auth` at `v1.4.0` with `br-core-auth/v0.1.0` (or the reverse) compiles but every handler gets a 500 `Missing request extension` or no passport.
