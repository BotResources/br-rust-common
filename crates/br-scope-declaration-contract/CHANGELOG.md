# Changelog

All notable changes to `br-scope-declaration-contract` are documented here. The git tag
`br-scope-declaration-contract/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-scope-declaration-contract` as its own library, tagged `br-scope-declaration-contract/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line. No source file differs from `v1.4.0`.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-scope-declaration-contract", tag = "br-scope-declaration-contract/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pin: this crate depends on `br-core-integration`, pinned by tag `br-core-integration/v0.1.0`, and returns its `CommandCoords`, `EventCoords` and `CoordError`, so a consumer resolves one copy of the types they share. Move this crate and `br-core-integration` from `v1.4.0` to their per-crate pins together: a `br-core-integration` at another tag next to it holds two `CommandCoords` types, and passing one to the other side fails to compile (E0308).
