# Changelog

All notable changes to `br-util-nats-fabric` are documented here. The git tag
`br-util-nats-fabric/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-util-nats-fabric` as its own library, tagged `br-util-nats-fabric/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line. No source file differs from `v1.4.0`.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-nats-fabric", tag = "br-util-nats-fabric/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Feature: `outbox` (the Postgres transactional outbox, off by default) is unchanged; pin it with `features = ["outbox"]` next to the tag and version.
- Sibling pins: this crate depends on `br-core-integration` and `br-core-events`, pinned by tag `br-core-integration/v0.1.0` and `br-core-events/v0.1.0`. It re-exports `EventMetadata` from `br-core-events` and `IntegrationEvent`, `IntegrationCommand`, `MessageOutcome` and the coordinate types (`Bc`, `Aggregate`, `Verb`, `PastFact`, `CommandCoords`, `EventCoords`, `CoordError`) from `br-core-integration`, and `br-core-integration` carries `br-core-kernel`'s `Actor`, `UserId` and `ServiceAccountId` through `EventMetadata`, so a consumer resolves one copy of the types they share. Move this crate together with `br-core-integration`, `br-core-events` and `br-core-kernel` from `v1.4.0` to their per-crate pins: this crate at `v0.1.0` next to any of them at another tag holds two copies of the same type, and passing a value of one to the other fails to compile (E0308).
