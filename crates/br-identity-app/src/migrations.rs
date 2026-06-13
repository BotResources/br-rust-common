use sqlx::PgPool;

use crate::error::AppError;

pub async fn migrate(pool: &PgPool) -> Result<(), AppError> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(|e| AppError::Persistence(sqlx::Error::from(e)))?;
    Ok(())
}
