#![cfg(feature = "migrate")]

use std::path::Path;
use std::str::FromStr;

use br_test_support::{require_test_db_url, unique_suffix};
use br_util_postgres::migrations_status;
use sqlx::PgPool;
use sqlx::migrate::Migrator;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const FIRST: i64 = 20260101000001;
const SECOND: i64 = 20260101000002;
const GHOST: i64 = 20991231235959;

const MIGRATIONS_TABLE_DDL: &str = "CREATE TABLE _sqlx_migrations (
    version BIGINT PRIMARY KEY,
    description TEXT NOT NULL,
    installed_on TIMESTAMPTZ NOT NULL DEFAULT now(),
    success BOOLEAN NOT NULL,
    checksum BYTEA NOT NULL,
    execution_time BIGINT NOT NULL
)";

async fn connect_to_schema(url: &str, schema: &str, max_connections: u32) -> PgPool {
    let options = PgConnectOptions::from_str(url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .options([("search_path", schema)]);

    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
        .expect("connect to the sandbox schema")
}

struct Sandbox {
    url: String,
    schema: String,
    pool: PgPool,
}

impl Sandbox {
    async fn open() -> Self {
        let url = require_test_db_url();
        let schema = format!("br_migstatus_{}", unique_suffix());

        let admin = PgPoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .expect("connect to TEST_DATABASE_URL");
        sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
            .execute(&admin)
            .await
            .expect("create the sandbox schema");
        admin.close().await;

        let pool = connect_to_schema(&url, &schema, 2).await;

        Sandbox { url, schema, pool }
    }

    async fn scratch_pool(&self) -> PgPool {
        connect_to_schema(&self.url, &self.schema, 1).await
    }

    async fn migrations_table_exists(&self) -> bool {
        let name: Option<String> =
            sqlx::query_scalar("SELECT to_regclass('_sqlx_migrations')::text")
                .fetch_one(&self.pool)
                .await
                .expect("probe the migrations table");
        name.is_some()
    }

    async fn execute(&self, sql: &str) {
        sqlx::query(sql)
            .execute(&self.pool)
            .await
            .unwrap_or_else(|e| panic!("statement failed: {sql}: {e}"));
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let url = self.url.clone();
        let schema = self.schema.clone();

        std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("build the sandbox cleanup runtime")
                .block_on(async move {
                    let Ok(admin) = PgPoolOptions::new().max_connections(1).connect(&url).await
                    else {
                        return;
                    };
                    let _ = sqlx::query(&format!("DROP SCHEMA \"{schema}\" CASCADE"))
                        .execute(&admin)
                        .await;
                    admin.close().await;
                });
        })
        .join()
        .expect("the sandbox cleanup thread must not panic");
    }
}

async fn embedded() -> Migrator {
    Migrator::new(Path::new("tests/migrations_fixture"))
        .await
        .expect("resolve the fixture migration set")
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a reachable Postgres"]
async fn fresh_database_reports_every_migration_pending_without_creating_the_table() {
    let sandbox = Sandbox::open().await;
    let migrator = embedded().await;

    let status = migrations_status(&sandbox.pool, &migrator)
        .await
        .expect("status against a database with no migrations table");

    assert_eq!(status.applied, 0);
    assert_eq!(status.pending, vec![FIRST, SECOND]);
    assert!(status.checksum_mismatch.is_empty());
    assert!(status.applied_not_embedded.is_empty());
    assert_eq!(status.dirty, None);
    assert!(!status.is_current());
    assert!(
        !sandbox.migrations_table_exists().await,
        "migrations_status must never create _sqlx_migrations"
    );
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a reachable Postgres"]
async fn empty_migrations_table_reports_every_migration_pending() {
    let sandbox = Sandbox::open().await;
    let migrator = embedded().await;
    sandbox.execute(MIGRATIONS_TABLE_DDL).await;

    let status = migrations_status(&sandbox.pool, &migrator)
        .await
        .expect("status against an empty migrations table");

    assert_eq!(status.applied, 0);
    assert_eq!(status.pending, vec![FIRST, SECOND]);
    assert!(!status.is_current());
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a reachable Postgres"]
async fn fully_migrated_database_is_current() {
    let sandbox = Sandbox::open().await;
    let migrator = embedded().await;
    migrator.run(&sandbox.pool).await.expect("run migrations");

    let status = migrations_status(&sandbox.pool, &migrator)
        .await
        .expect("status against a fully migrated database");

    assert_eq!(status.applied, 2);
    assert!(status.pending.is_empty());
    assert!(status.checksum_mismatch.is_empty());
    assert!(status.applied_not_embedded.is_empty());
    assert_eq!(status.dirty, None);
    assert!(status.embedded_applied());
    assert!(status.is_current());
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a reachable Postgres"]
async fn edited_migration_surfaces_as_a_checksum_mismatch() {
    let sandbox = Sandbox::open().await;
    let migrator = embedded().await;
    migrator.run(&sandbox.pool).await.expect("run migrations");

    sqlx::query("UPDATE _sqlx_migrations SET checksum = $1 WHERE version = $2")
        .bind(vec![0u8; 48])
        .bind(FIRST)
        .execute(&sandbox.pool)
        .await
        .expect("tamper with the stored checksum");

    let status = migrations_status(&sandbox.pool, &migrator)
        .await
        .expect("status against a tampered migration row");

    assert_eq!(status.applied, 2);
    assert!(status.pending.is_empty());
    assert_eq!(status.checksum_mismatch, vec![FIRST]);
    assert!(!status.is_current());
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a reachable Postgres"]
async fn database_ahead_of_the_binary_is_reported_and_is_not_current() {
    let sandbox = Sandbox::open().await;
    let migrator = embedded().await;
    migrator.run(&sandbox.pool).await.expect("run migrations");

    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES ($1, 'applied by a newer binary', true, $2, 0)",
    )
    .bind(GHOST)
    .bind(vec![7u8; 48])
    .execute(&sandbox.pool)
    .await
    .expect("insert a migration this binary does not embed");

    let status = migrations_status(&sandbox.pool, &migrator)
        .await
        .expect("status against a database ahead of the binary");

    assert_eq!(status.applied, 2);
    assert!(status.pending.is_empty());
    assert!(status.checksum_mismatch.is_empty());
    assert_eq!(status.applied_not_embedded, vec![GHOST]);
    assert!(status.embedded_applied());
    assert!(!status.is_current());

    let scratch = sandbox.scratch_pool().await;
    let boot = migrator.run(&scratch).await;
    scratch.close().await;
    assert!(
        matches!(
            boot,
            Err(sqlx::migrate::MigrateError::VersionMissing(GHOST))
        ),
        "a strict migrator must refuse to boot against a database ahead of it, got: {boot:?}"
    );

    let mut lenient = embedded().await;
    lenient.set_ignore_missing(true);
    lenient
        .run(&sandbox.pool)
        .await
        .expect("a migrator built with set_ignore_missing(true) still boots");
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a reachable Postgres"]
async fn partially_applied_migration_is_reported_dirty() {
    let sandbox = Sandbox::open().await;
    let migrator = embedded().await;
    migrator.run(&sandbox.pool).await.expect("run migrations");

    sqlx::query("UPDATE _sqlx_migrations SET success = false WHERE version = $1")
        .bind(SECOND)
        .execute(&sandbox.pool)
        .await
        .expect("mark a migration as partially applied");

    let status = migrations_status(&sandbox.pool, &migrator)
        .await
        .expect("status against a dirty database");

    assert_eq!(status.dirty, Some(SECOND));
    assert!(status.pending.is_empty());
    assert!(!status.is_current());
}
