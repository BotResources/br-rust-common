# Changelog

All notable changes to `br-core-scope` are documented here. The git tag
`br-core-scope/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.1

### Added

- **Scope implications and service-account-only scopes, as a v2 declaration.** `ScopeSpecV2` carries the `ScopeSpec` fields plus `implies: Vec<ScopeKey>` (other scopes of the same declaration this scope implies) and `service_accounts_only: bool` (the scope can only be granted to service accounts); both default on deserialization (`[]`, `false`), so a minimal v2 payload parses. `ScopeSpecV2` is `#[non_exhaustive]`: build it with `ScopeSpecV2::new(..)`, `with_implies(..)` and `with_service_accounts_only(..)`.
- `ScopeDeclarationV2`, validated atomically by `new` and on deserialization: the v1 rules (scope owned by the declaring service, no duplicate), then every implied key must belong to the declaring service and be declared in the same declaration, and the implication graph must have no cycle (a self-implication is a cycle). `From<ScopeDeclaration>` lifts a v1 declaration with the defaults.
- `RawScopeDeclarationV2` / `RawScopeSpecV2` (receiver-side raw form, `validate()` gives the structured reason, a malformed implied key is reported as `InvalidScopeKey`) and the command payload `DeclareServiceScopesV2`, carried on `integration.cmd.identity.service_scope.declare.v2`. The replies stay `ServiceScopesAccepted` / `ServiceScopesRejected`.
- Three rejection reasons on `ScopeDeclarationError` (`#[non_exhaustive]`, so matching code keeps compiling): `UnknownImpliedScope { key, implied }` (`unknown_implied_scope`), `ImpliedScopePrefixMismatch { key, implied, declaring_service }` (`implied_scope_prefix_mismatch`), `ImplicationCycle { cycle }` (`implication_cycle`; `cycle` lists the keys along the cycle, the first key repeated at the end).

### Unchanged

- The v1 types (`ScopeSpec`, `ScopeDeclaration`, `RawScopeDeclaration`, `RawScopeSpec`, `DeclareServiceScopes`) and their wire bytes; a test pins the v1 declare payload byte for byte.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-core-scope", tag = "br-core-scope/v0.1.1", version = "0.1.1" }`.

## 0.1.0

### Changed

- First release of `br-core-scope` as its own library, tagged `br-core-scope/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-core-scope", tag = "br-core-scope/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
