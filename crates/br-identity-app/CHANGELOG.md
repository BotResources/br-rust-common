# Changelog

All notable changes to `br-identity-app` are documented here. The git tag
`br-identity-app/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-identity-app` as its own library, tagged `br-identity-app/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line. No source file differs from `v1.4.0`.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-identity-app", tag = "br-identity-app/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pins: this crate depends on `br-core-integration`, `br-core-scope`, `br-identity-domain`, `br-scope-declaration-contract` and `br-util-nats-fabric`, pinned by tag `br-core-integration/v0.1.0`, `br-core-scope/v0.1.0`, `br-identity-domain/v0.1.0`, `br-scope-declaration-contract/v0.1.0` and `br-util-nats-fabric/v0.1.0`. It names `Fabric` and `FabricError` from `br-util-nats-fabric`, `IntegrationCommand` and `CommandCoords` from `br-core-integration`, `ServiceKey`, `DeclareServiceScopes` and `ScopeDeclarationError` from `br-core-scope`, and `ScopeRegistry`, `DeclarationOutcome` and `RegistryHydrationError` from `br-identity-domain`, so a consumer resolves one copy of the types they share. Move this crate together with those siblings and `br-core-events` and `br-core-kernel` from `v1.4.0` to their per-crate pins: this crate at `v0.1.0` next to any of them at another tag (such as `br-core-scope` at `v1.4.0`) holds two copies of the same type, and passing a value of one to the other fails to compile (E0308).
