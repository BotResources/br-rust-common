# br-test-support

Dev-only shared **Postgres e2e test helpers** for the `br-rust-common` crates.
Role/pool/name primitives that several crates' integration tests need to stand
up an ephemeral, least-privilege Postgres world (create a `CREATEROLE` caller,
open a pool as a given role, generate collision-free role/table names, tear a
role down). Tier `util` — functions only, no aggregate, no policy, no error type
of its own (the helpers surface `sqlx::Error` directly).

These helpers used to be copy-pasted across three sites
(`br-util-postgres/src/test_support.rs`, `br-util-postgres/tests/common`,
`br-identity-app/tests/common`). This crate is the single home; the consumers
import it.

## Dev-dependency only — never a normal dependency

This crate exists **only to support tests**. It MUST be referenced exclusively
under `[dev-dependencies]`, never `[dependencies]`, and it never appears in any
crate's public API. Cargo does not pull a dependency's dev-dependencies
transitively, so a downstream git-rev consumer of `br-rust-common` (e.g.
be-botresources) never builds, links, or sees `br-test-support`. Promoting it to
a normal dependency would break that guarantee — don't.

## Surface

| Item | Role |
|---|---|
| `test_db_url() -> Option<String>` | Reads `TEST_DATABASE_URL` (a PG 16+ superuser/admin URL). `None` → the calling `#[ignore]` test self-skips. |
| `test_tls_db_url() -> Option<String>` | Reads `TEST_TLS_DATABASE_URL` (a TLS-required PG URL) for the remote-TLS path. |
| `unique_suffix() -> String` | A 24-char collision-free suffix (truncated UUIDv7 simple form) for building per-test object names. |
| `unique_role_name() -> String` | `br_test_{suffix}` — a fresh role name for one test. |
| `unique_table_name() -> String` | `br_test_tbl_{suffix}` — a fresh table name for one test. |
| `cleanup_role(admin, role)` | Best-effort teardown: `DROP OWNED BY … CASCADE` then `DROP ROLE IF EXISTS …`. Errors are swallowed (teardown is idempotent and runs after the assertions). |
| `setup_caller(admin, admin_url) -> (PgPool, String)` | Creates a fresh `LOGIN CREATEROLE NOSUPERUSER` role, GRANTs `CREATE ON SCHEMA public`, and returns a pool already authenticated as it plus the role name. Use as the owner/caller that the code under test will itself create app roles from. |
| `open_pool_as(admin_url, role, password) -> Result<PgPool, sqlx::Error>` | Opens a pool against `admin_url`'s host/db but authenticated as `role`/`password`. Returns the error so a test can assert a failed connection. |

All pools cap at `max_connections(2)` — enough for a single e2e test, deliberately
small so a leaked pool surfaces fast.

## Why

| Thing | Why it is the way it is |
|---|---|
| Dev-dependency only | Test scaffolding must not enter any crate's dependency closure or public API; dev-deps are not transitive, so consumers stay unaffected (issue #47). |
| `cleanup_role` swallows errors | It is post-assertion teardown of an already-ephemeral role; failing it would mask the real test outcome, and `DROP … IF EXISTS` is idempotent. |
| Passwords are inline literals | These roles live only for the duration of one e2e test against a disposable database; they are never real credentials. |
| `expect(...)` on setup, `Result` on `open_pool_as` | Setup failures are environment faults that should abort the test loudly; `open_pool_as` is also used to *assert* that a connection is refused, so it must return the error rather than panic. |

## Usage

This crate is **dev-only and workspace-internal**. It is not published and must
never appear in the dependency closure of any production crate. Reference it only
from `[dev-dependencies]` of crates that live inside the `br-rust-common`
workspace, using a path dependency:

```toml
[dev-dependencies]
br-test-support = { path = "../br-test-support" }
```

External consumers of `br-rust-common` must **never** depend on this crate.

---

Part of [`br-rust-common`](https://github.com/BotResources/br-rust-common). See
[CHANGELOG.md](../../CHANGELOG.md). © BotResources — MIT.
