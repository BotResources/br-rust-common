#![cfg(feature = "consumer")]

mod support;

use std::path::PathBuf;

use sqlx::PgPool;
use sqlx::migrate::{Migration, MigrationType, Migrator};

use support::bare_pool;

struct ImporterMigrations {
    dir: PathBuf,
}

impl ImporterMigrations {
    fn new(files: &[(&str, &str)]) -> Self {
        let dir =
            std::env::temp_dir().join(format!("importer_{}", br_test_support::unique_suffix()));
        std::fs::create_dir_all(&dir).expect("create the importer migrations directory");
        for (name, sql) in files {
            std::fs::write(dir.join(name), sql).expect("write an importer migration");
        }
        Self { dir }
    }

    async fn run(&self, pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
        Migrator::new(self.dir.as_path()).await?.run(pool).await
    }
}

impl Drop for ImporterMigrations {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn importer() -> ImporterMigrations {
    ImporterMigrations::new(&[
        (
            "0001_importer_orders.sql",
            "CREATE TABLE importer_orders (id uuid PRIMARY KEY)",
        ),
        (
            "0002_importer_lines.sql",
            "CREATE TABLE importer_lines (id uuid PRIMARY KEY)",
        ),
    ])
}

async fn table_exists(pool: &PgPool, name: &str) -> bool {
    sqlx::query_scalar::<_, bool>("SELECT to_regclass($1) IS NOT NULL")
        .bind(name)
        .fetch_one(pool)
        .await
        .expect("look the table up")
}

async fn directory_tables_exist(pool: &PgPool) -> bool {
    for table in [
        "known_users",
        "known_groups",
        "known_user_group",
        "known_service_accounts",
    ] {
        if !table_exists(pool, table).await {
            return false;
        }
    }
    true
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
async fn migrate_runs_on_a_database_the_importer_already_migrated_with_its_own_set() {
    let pool = bare_pool(&br_test_support::require_test_db_url()).await;
    importer().run(&pool).await.expect("importer migrations");

    br_util_directory::migrate(&pool)
        .await
        .expect("directory migrate beside the importer ledger");

    assert!(directory_tables_exist(&pool).await);
    assert!(table_exists(&pool, "importer_orders").await);
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
async fn migrate_is_repeatable_beside_the_importer_ledger() {
    let pool = bare_pool(&br_test_support::require_test_db_url()).await;
    importer().run(&pool).await.expect("importer migrations");

    br_util_directory::migrate(&pool).await.expect("first run");
    br_util_directory::migrate(&pool).await.expect("second run");
    br_util_directory::migrate(&pool).await.expect("third run");

    assert!(directory_tables_exist(&pool).await);
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
async fn the_importer_migrator_still_runs_after_the_directory_migrated() {
    let pool = bare_pool(&br_test_support::require_test_db_url()).await;
    let importer = importer();

    br_util_directory::migrate(&pool)
        .await
        .expect("directory migrate");
    importer
        .run(&pool)
        .await
        .expect("importer migrations after the directory");
    importer
        .run(&pool)
        .await
        .expect("importer migrations again");
    br_util_directory::migrate(&pool)
        .await
        .expect("directory migrate again");

    assert!(directory_tables_exist(&pool).await);
    assert!(table_exists(&pool, "importer_lines").await);
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
async fn concurrent_migrations_converge_without_error() {
    let pool = bare_pool(&br_test_support::require_test_db_url()).await;
    importer().run(&pool).await.expect("importer migrations");

    let runs: Vec<_> = (0..4)
        .map(|_| {
            let pool = pool.clone();
            tokio::spawn(async move { br_util_directory::migrate(&pool).await })
        })
        .collect();
    for run in runs {
        run.await.expect("join").expect("concurrent migrate");
    }

    assert!(directory_tables_exist(&pool).await);
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
async fn migrate_adopts_a_database_the_previous_release_already_migrated() {
    let pool = bare_pool(&br_test_support::require_test_db_url()).await;
    let legacy = include_str!("../migrations/0001_known_directory.sql");
    sqlx::raw_sql(legacy)
        .execute(&pool)
        .await
        .expect("create the legacy known_* tables");
    sqlx::raw_sql(
        "CREATE TABLE _sqlx_migrations (\
            version BIGINT PRIMARY KEY, description TEXT NOT NULL, \
            installed_on TIMESTAMPTZ NOT NULL DEFAULT now(), success BOOLEAN NOT NULL, \
            checksum BYTEA NOT NULL, execution_time BIGINT NOT NULL)",
    )
    .execute(&pool)
    .await
    .expect("create the legacy ledger");
    let checksum = Migration::new(
        1,
        "known directory".into(),
        MigrationType::Simple,
        legacy.into(),
        false,
    )
    .checksum;
    sqlx::query(
        "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time) \
         VALUES (1, 'known directory', true, $1, 0)",
    )
    .bind(checksum.as_ref())
    .execute(&pool)
    .await
    .expect("record the legacy migration");

    br_util_directory::migrate(&pool)
        .await
        .expect("adopt the legacy database");
    br_util_directory::migrate(&pool)
        .await
        .expect("run again after adoption");

    assert!(directory_tables_exist(&pool).await);
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL pointing at a real Postgres"]
async fn migrate_refuses_a_recorded_migration_whose_checksum_drifted() {
    let pool = bare_pool(&br_test_support::require_test_db_url()).await;
    br_util_directory::migrate(&pool).await.expect("first run");
    sqlx::query("UPDATE _br_directory_migrations SET checksum = '\\x00' WHERE version = 1")
        .execute(&pool)
        .await
        .expect("tamper with the recorded checksum");

    let outcome = br_util_directory::migrate(&pool).await;

    assert!(matches!(
        outcome,
        Err(br_util_directory::DirectoryError::Migrate(
            sqlx::migrate::MigrateError::VersionMismatch(1)
        ))
    ));
}
