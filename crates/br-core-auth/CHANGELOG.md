# Changelog

All notable changes to `br-core-auth` are documented here. The git tag
`br-core-auth/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-core-auth` as its own library, tagged `br-core-auth/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-core-auth", tag = "br-core-auth/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pins: this crate depends on `br-core-kernel` and `br-core-scope`, pinned by tag `br-core-kernel/v0.1.0` and `br-core-scope/v0.1.0`, so a consumer resolves one copy of the types they share. Move this crate and the siblings it names from `v1.4.0` to their per-crate pins together.
- Take `br-core-auth` and `br-util-axum-auth` at `0.1.0` together: `br-util-axum-auth` inserts this crate's `Passport` into the request extensions, keyed by type. A service that mounts `br-util-axum-auth` at `v1.4.0` while `br-core-auth` is `0.1.0` compiles, but every handler gets a 500 `Missing request extension` or no passport; such a service keeps `br-core-auth`, `br-core-kernel` and `br-core-scope` on `v1.4.0` until it moves `br-util-axum-auth` to `0.1.0`.
