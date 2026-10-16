use sqlx::migrate::{MigrateError, Migration};
use sqlx::{Executor, PgConnection, Row};

use crate::error::DirectoryError;

const LEGACY_LEDGER_TABLE: &str = "_sqlx_migrations";
const LEDGER_LOCK_KEY: i64 = 0x62_72_64_69_72_00_01;

pub(super) async fn lock(conn: &mut PgConnection) -> Result<(), DirectoryError> {
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(LEDGER_LOCK_KEY)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub(super) async fn ensure_table(conn: &mut PgConnection) -> Result<(), DirectoryError> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS _br_directory_migrations (\
            version bigint PRIMARY KEY, \
            description text NOT NULL, \
            checksum bytea NOT NULL, \
            applied_at timestamptz NOT NULL DEFAULT now())",
    )
    .execute(&mut *conn)
    .await?;
    Ok(())
}

pub(super) async fn apply_pending(
    conn: &mut PgConnection,
    migrations: &[Migration],
) -> Result<(), DirectoryError> {
    let recorded = recorded_checksums(conn).await?;
    for migration in migrations {
        match recorded.iter().find(|(v, _)| *v == migration.version) {
            Some((_, checksum)) if checksum.as_slice() == migration.checksum.as_ref() => {}
            Some(_) => return Err(MigrateError::VersionMismatch(migration.version).into()),
            None if adopted_from_legacy(conn, migration).await? => record(conn, migration).await?,
            None => {
                conn.execute(migration.sql.as_ref()).await?;
                record(conn, migration).await?;
            }
        }
    }
    Ok(())
}

async fn recorded_checksums(
    conn: &mut PgConnection,
) -> Result<Vec<(i64, Vec<u8>)>, DirectoryError> {
    let rows = sqlx::query("SELECT version, checksum FROM _br_directory_migrations")
        .fetch_all(&mut *conn)
        .await?;
    Ok(rows
        .iter()
        .map(|row| (row.get("version"), row.get("checksum")))
        .collect())
}

async fn adopted_from_legacy(
    conn: &mut PgConnection,
    migration: &Migration,
) -> Result<bool, DirectoryError> {
    let has_legacy: bool = sqlx::query_scalar("SELECT to_regclass($1) IS NOT NULL")
        .bind(LEGACY_LEDGER_TABLE)
        .fetch_one(&mut *conn)
        .await?;
    if !has_legacy {
        return Ok(false);
    }
    let adopted: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM _sqlx_migrations \
         WHERE version = $1 AND checksum = $2 AND success)",
    )
    .bind(migration.version)
    .bind(migration.checksum.as_ref())
    .fetch_one(&mut *conn)
    .await?;
    Ok(adopted)
}

async fn record(conn: &mut PgConnection, migration: &Migration) -> Result<(), DirectoryError> {
    sqlx::query(
        "INSERT INTO _br_directory_migrations (version, description, checksum) VALUES ($1, $2, $3)",
    )
    .bind(migration.version)
    .bind(migration.description.as_ref())
    .bind(migration.checksum.as_ref())
    .execute(&mut *conn)
    .await?;
    Ok(())
}
