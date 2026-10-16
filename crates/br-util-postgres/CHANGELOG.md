# Changelog

All notable changes to `br-util-postgres` are documented here. The git tag
`br-util-postgres/v{version}` releases the crate; this crate is versioned
separately from the other crates of the repository. Format follows
[Keep a Changelog](https://keepachangelog.com/); versions follow semver.
Release headings are plain `## X.Y.Z` — the release pipeline greps that exact
form to decide whether a version ships.

## Unreleased

## 0.1.1

### Changed

- **`ensure_app_role` refuses an empty password.** It returns `PostgresError::Config` before any statement is sent; it used to clear the role password silently. A password containing a NUL byte is refused the same way.
- **The stored secret is always SCRAM-SHA-256**, whatever `password_encryption` says, because the crate sends a verifier and Postgres stores it as is. Clients without SCRAM support cannot log in as this role.
- **The verifier uses a fixed iteration count of 4096.** The `ALTER ROLE` runs as a non-persistent statement, so sqlx does not keep the verifier in its statement cache.
- **A server-side password policy hook such as `passwordcheck` cannot see the plain password**, because Postgres only receives the verifier.

### Fixed

- **`ensure_app_role` no longer fails when several callers create the same role at once.** The check-then-create `DO` block had no lock and no retry, so parallel first calls raced on `CREATE ROLE` (`42710 duplicate_object` or a `pg_authid_rolname_index` unique violation) and on `ALTER ROLE ... PASSWORD` (`XX000 tuple concurrently updated`). The create and the password change now run in one transaction under a per-role `pg_advisory_xact_lock`, which serialises callers of one database. Callers on different databases of one cluster, which an advisory lock cannot order, retry up to 6 attempts with a short backoff on exactly these three SQLSTATEs; any other error, and the last failed attempt, is returned as `PostgresError::Db`. The public signature is unchanged. `tokio` is a new direct dependency, `time` feature only.
- **`grant_app_access` no longer fails when several callers grant at once.** Its five `GRANT` / `ALTER DEFAULT PRIVILEGES` statements ran autocommit with no lock and no retry, so replicas starting together on a fresh database failed with `XX000 tuple concurrently updated` or a `pg_default_acl_role_nsp_obj_index` unique violation, including when two different app roles were granted at once. The five statements now run in one transaction under one database-wide `pg_advisory_xact_lock` (its own key namespace, distinct from the `ensure_app_role` one), with the same bounded retry as `ensure_app_role` for callers the lock cannot order; the retry helper is shared by both functions. The public signature and the error type are unchanged.

### Security

- **`ensure_app_role` no longer sends the clear password to Postgres.** It used to build ``ALTER ROLE "r" PASSWORD $tag$clear$tag$``, so the clear password sat in the statement text (server logs, `pg_stat_statements`, the wire). It now computes a SCRAM-SHA-256 verifier in the process (`postgres-protocol`, random 16-byte salt, 4096 iterations) and sends only ``ALTER ROLE "r" PASSWORD 'SCRAM-SHA-256$4096:...'``; `pg_authid.rolpassword` holds the verifier. The verifier is checked against the alphabet `[A-Za-z0-9+/=$:-]` before it is quoted, and a failed check is `PostgresError::Config`. The dollar-quote machinery and the buffer scrub are removed. The new dependency is `postgres-protocol` `0.6.12` or later, the first release outside RUSTSEC-2026-0179 and RUSTSEC-2026-0180; it adds `fallible-iterator` and the RustCrypto 0.11 line (`hmac`, `sha2`, `md-5`, `digest` and their small helper crates) next to the 0.10 line `sqlx` already uses.

## 0.1.0

### Changed

- First release of `br-util-postgres` as its own library, tagged `br-util-postgres/v0.1.0`. The code is the one shipped by the workspace release `v1.4.0` plus the DSN host fix below, which `v1.4.0` does not contain; the versioning also changes: the single workspace version and the single `vX.Y.Z` tag are gone, and this crate now carries its own version line.
- Dependency pin: `{ git = "https://github.com/BotResources/br-rust-common", package = "br-util-postgres", tag = "br-util-postgres/v0.1.0", version = "0.1.0" }`. Moving from `tag = "v1.4.0"`, change both the tag and the version: the number goes down because the line restarts at `0.1`.
- Sibling pin: this crate depends on `br-util-boot` pinned by tag `br-util-boot/v0.1.0`, so the two crates agree on one `DATABASE_URL` name constant and one resolved `br-util-boot`. They share no type. Move this crate and `br-util-boot` from `v1.4.0` to their per-crate pins together.
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
  accepted only when sqlx can parse it, and a host listed in `TRUSTED_NETWORK_HOSTS` is no exception: a trusted host with an invalid `sslmode` is now a `Config` error. A non-ASCII trusted host no longer matches, because the URL parser percent-encodes the host. IPv6 hosts compare in canonical
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
