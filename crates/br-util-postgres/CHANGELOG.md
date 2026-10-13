# Changelog

All notable changes to `br-util-postgres` are documented here. The git tag
`br-util-postgres/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.0

### Changed

- First release of `br-util-postgres` as its own library, tagged `br-util-postgres/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0` plus the DSN host fix below, which `v1.4.0` does not contain; the versioning also changes: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pin: this crate depends on `br-util-boot` pinned by tag `br-util-boot/v0.1.0`, so a consumer resolves one copy of the types they share. Move this crate and `br-util-boot` from `v1.4.0` to their per-crate pins together.
- **`br-util-postgres` — `validate_database_tls` is stricter about what it
  accepts.** `v1.4.0` does not contain these rules. These DSNs were accepted
  before and now return `PostgresError::Config` before any trust decision: a
  DSN with no scheme (`localhost`, `localhost/db`), a foreign scheme
  (`mysql://`, `pg://`), a DSN with no explicit host (`postgres:///db?sslmode=require`),
  whose host the driver would take from `PGHOSTADDR`, `PGHOST` or a socket
  directory (such a DSN must now name its host), and any DSN that sqlx cannot
  parse, whatever its host: a DSN with an invalid `sslmode` value
  (`?sslmode=bogus`) or an invalid port, which `v1.4.0` accepted when the host
  was loopback, is now refused before the loopback shortcut. A loopback DSN is
  accepted only when sqlx can parse it. IPv6 hosts compare in canonical
  lower-case form (`[0:0:0:0:0:0:0:1]` is `::1`). An upper-case scheme
  (`POSTGRES://`) is now accepted.

### Fixed

- **`br-util-postgres` — `validate_database_tls` no longer reads the host from
  outside the URL authority (fail-open parsing).** The host was taken after the
  first `@` of the whole DSN, so a DSN with `@localhost` only in its query,
  path or fragment (for example `postgres://evil.example.com/db?x=a@localhost`)
  was judged `localhost`: treated as loopback and accepted without TLS, while
  the driver connected to `evil.example.com`. The host is now read from the
  authority only (credentials end at the last `@` of the authority; a `@` in the
  path, query or fragment, or percent-encoded in the user information, never
  changes it), with the same URL parser the driver uses. `init_pool` and
  `init_migration_pool` inherit the fix. The public signatures are unchanged.
