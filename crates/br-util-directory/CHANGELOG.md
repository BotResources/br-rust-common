# Changelog

All notable changes to `br-util-directory` are documented here. The git tag
`br-util-directory/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.1

### Fixed

- **`migrate` no longer fails on a database whose `_sqlx_migrations` ledger holds another migration set.** It ran `sqlx::migrate!` against the importer's own ledger, so an importer with its own migrations got `VersionMissing` (the ledger held versions this crate does not know) or `VersionMismatch` (the same version number, another checksum), whichever side migrated first, and the importer's own migrator failed on the crate's version `1` in the same way. `migrate` now keeps its own ledger table `_br_directory_migrations`; it never writes the importer's ledger, and reads it only to adopt a row written by `0.1.0`, under one transaction-scoped advisory lock so concurrent callers converge. A recorded migration whose checksum drifted still fails, with `DirectoryError::Migrate(VersionMismatch)`. The public signature and the error type are unchanged.
- **A database migrated by `0.1.0` is adopted without re-running the SQL.** When `_sqlx_migrations` holds this crate's migration with the same checksum, `migrate` records it in the new ledger and leaves the old row in place. An importer whose own migrator still lists that row can drop it, or keep ignoring missing versions.

### Changed

- Dependency pin: `br-util-postgres` moves from `br-util-postgres/v0.1.0` to `br-util-postgres/v0.1.1`. `br-util-postgres` types are not part of this crate's API.

## 0.1.0

### Changed

- First release of `br-util-directory` as its own library, tagged `br-util-directory/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line. No source file differs from `v1.4.0`.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-directory", tag = "br-util-directory/v0.1.0", version = "0.1.0", features = ["consumer"] }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Features: `publisher` and `consumer` (both off by default) are unchanged; pin them with `features = ["publisher"]` (the identity side, which writes the roster), `features = ["consumer"]` (the `known_*` projection and readers) or both next to the tag and version.
- Sibling pins: this crate depends on `br-core-directory`, `br-util-nats-fabric` and, behind the `consumer` feature, `br-util-postgres`, pinned by tag `br-core-directory/v0.1.0`, `br-util-nats-fabric/v0.1.0` and `br-util-postgres/v0.1.0`. It names `PublishedUser`, `PublishedGroup`, `PublishedServiceAccount` and `DirectoryMeta` from `br-core-directory`, and `Fabric`, `FabricError`, `KvKeyError`, `ProjectionError` and `WatchHealthReceiver` from `br-util-nats-fabric`, so a consumer resolves one copy of the types they share. `connect_pool` delegates to `br_util_postgres::init_pool`, so a consumer moving from `v1.4.0` meets the stricter DSN rules of `br-util-postgres/v0.1.0` through it: see the `0.1.0` entry of [`br-util-postgres`](../br-util-postgres/CHANGELOG.md). No type of `br-util-postgres` appears in its public API. Move this crate together with `br-core-directory` and `br-util-nats-fabric` (and, through the fabric, `br-core-integration`, `br-core-events` and `br-core-kernel`) from `v1.4.0` to their per-crate pins: this crate at `v0.1.0` next to any of them at another tag holds two copies of the same type, and passing a value of one to the other fails to compile (E0308).
