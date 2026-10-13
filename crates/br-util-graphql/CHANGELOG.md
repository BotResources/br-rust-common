# Changelog

All notable changes to `br-util-graphql` are documented here. The git tag
`br-util-graphql/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-util-graphql` as its own library, tagged `br-util-graphql/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0`; what changes is the versioning: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-graphql", tag = "br-util-graphql/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pin: this crate depends on `br-core-values`, pinned by tag `br-core-values/v0.1.0`, so a consumer resolves one copy of the types they share. Move this crate and the siblings it names from `v1.4.0` to their per-crate pins together.
