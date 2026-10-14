# Changelog

All notable changes to `br-util-directory` are documented here. The git tag
`br-util-directory/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-util-directory` as its own library, tagged `br-util-directory/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line. No source file differs from `v1.4.0`.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-directory", tag = "br-util-directory/v0.1.0", version = "0.1.0", features = ["consumer"] }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Features: `publisher` and `consumer` (both off by default) are unchanged; pin them with `features = ["publisher"]` (the identity side, which writes the roster), `features = ["consumer"]` (the `known_*` projection and readers) or both next to the tag and version.
- Sibling pins: this crate depends on `br-core-directory`, `br-util-nats-fabric` and, behind the `consumer` feature, `br-util-postgres`, pinned by tag `br-core-directory/v0.1.0`, `br-util-nats-fabric/v0.1.0` and `br-util-postgres/v0.1.0`. It names `PublishedUser`, `PublishedGroup`, `PublishedServiceAccount` and `DirectoryMeta` from `br-core-directory`, and `Fabric`, `FabricError`, `KvKeyError`, `ProjectionError` and `WatchHealthReceiver` from `br-util-nats-fabric`, so a consumer resolves one copy of the types they share. No type of `br-util-postgres` appears in its public API. Move this crate together with `br-core-directory` and `br-util-nats-fabric` (and, through the fabric, `br-core-integration`, `br-core-events` and `br-core-kernel`) from `v1.4.0` to their per-crate pins: this crate at `v0.1.0` next to any of them at another tag holds two copies of the same type, and passing a value of one to the other fails to compile (E0308).
