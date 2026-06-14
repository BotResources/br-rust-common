use sqlx::PgPool;

use crate::error::DirectoryError;

pub async fn connect_pool(database_url: &str) -> Result<PgPool, DirectoryError> {
    br_util_postgres::init_pool(database_url)
        .await
        .map_err(|e| DirectoryError::Pool(e.to_string()))
}

pub async fn migrate(pool: &PgPool) -> Result<(), DirectoryError> {
    sqlx::migrate!("./migrations").run(pool).await?;
    Ok(())
}
