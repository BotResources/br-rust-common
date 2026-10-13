# Changelog

All notable changes to `br-util-postgres` are documented here. The git tag
`br-util-postgres/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

v1.4.0 does not contain the fix below; it ships with the crate's first release on its own line.

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

### Changed

- **`br-util-postgres` — `validate_database_tls` is stricter about what it
  accepts.** These DSNs were accepted before and now return
  `PostgresError::Config` before any trust check: a DSN with no scheme
  (`localhost`, `localhost/db`), a foreign scheme (`mysql://`, `pg://`), and a
  DSN with no explicit host (`postgres:///db?sslmode=require`, or
  `?sslmode=bogus`), whose host the driver would take from `PGHOSTADDR`,
  `PGHOST` or a socket directory: such a DSN must now name its host. IPv6 hosts
  compare in canonical lower-case form (`[0:0:0:0:0:0:0:1]` is `::1`). An
  upper-case scheme (`POSTGRES://`) is now accepted.
