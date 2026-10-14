# Changelog

All notable changes to `br-util-scope-declaration` are documented here. The git tag
`br-util-scope-declaration/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-util-scope-declaration` as its own library, tagged `br-util-scope-declaration/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line. No source file differs from `v1.4.0`.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-scope-declaration", tag = "br-util-scope-declaration/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pins: this crate depends on `br-core-integration`, `br-core-scope`, `br-scope-declaration-contract`, `br-util-axum-readiness` and `br-util-nats-fabric`, pinned by tag `br-core-integration/v0.1.0`, `br-core-scope/v0.1.0`, `br-scope-declaration-contract/v0.1.0`, `br-util-axum-readiness/v0.1.0` and `br-util-nats-fabric/v0.1.0`. It names `Fabric` and `FabricError` from `br-util-nats-fabric`, `ScopeDeclaration`, `ServiceKey` and `ServiceScopesRejected` from `br-core-scope`, `ReadinessHandle` from `br-util-axum-readiness`, and returns `Actor`, which is `br-core-kernel`'s type carried by `br-core-integration` and `br-core-events`, so a consumer resolves one copy of the types they share. Move this crate together with those siblings and `br-core-events` and `br-core-kernel` from `v1.4.0` to their per-crate pins: this crate at `v0.1.0` next to any of them at another tag holds two copies of the same type, and passing a value of one to the other fails to compile (E0308).
