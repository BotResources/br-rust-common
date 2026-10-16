use sqlx::PgPool;

use crate::error::DirectoryError;

use super::ledger;

pub async fn connect_pool(database_url: &str) -> Result<PgPool, DirectoryError> {
    br_util_postgres::init_pool(database_url)
        .await
        .map_err(|e| DirectoryError::Pool(e.to_string()))
}

pub async fn migrate(pool: &PgPool) -> Result<(), DirectoryError> {
    let migrator = sqlx::migrate!("./migrations");
    let migrations: Vec<_> = migrator
        .iter()
        .filter(|migration| !migration.migration_type.is_down_migration())
        .cloned()
        .collect();
    let mut tx = pool.begin().await?;
    ledger::lock(&mut tx).await?;
    ledger::ensure_table(&mut tx).await?;
    ledger::apply_pending(&mut tx, &migrations).await?;
    tx.commit().await?;
    Ok(())
}
