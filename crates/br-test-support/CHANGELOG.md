# Changelog — br-test-support

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-06-13

**Added**
- Initial release. Dev-only shared **Postgres e2e test helpers** (tier `util`,
  functions only, no domain) extracted from three copy-pasted sites to kill the
  DRY breach (issue #47). Consumed exclusively as a **path dev-dependency** —
  never a normal dependency, never in any crate's public API; dev-deps are not
  transitive, so downstream git-rev consumers of `br-rust-common` are unaffected.
  - Env readers: `test_db_url`, `test_tls_db_url`.
  - Name generators: `unique_suffix`, `unique_role_name`, `unique_table_name`.
  - Role/pool primitives: `cleanup_role`, `setup_caller`, `open_pool_as`.
- Migrated `br-util-postgres` (its `#[cfg(test)]` unit tests and its
  `tests/common`) and `br-identity-app` (`tests/common`) onto this crate;
  removed the duplicated copies.
