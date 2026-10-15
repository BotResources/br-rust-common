use std::future::Future;
use std::time::Duration;

use sqlx::error::DatabaseError;

use crate::error::PostgresError;

const MAX_ATTEMPTS: u32 = 6;
const BACKOFF_STEP: Duration = Duration::from_millis(25);

const DUPLICATE_OBJECT: &str = "42710";
const UNIQUE_VIOLATION: &str = "23505";
const INTERNAL_ERROR: &str = "XX000";

pub(crate) fn is_concurrent_catalog_ddl(error: &dyn DatabaseError) -> bool {
    matches!(
        error.code().as_deref(),
        Some(DUPLICATE_OBJECT | UNIQUE_VIOLATION | INTERNAL_ERROR)
    )
}

pub(crate) async fn retry_concurrent_catalog_ddl<F, Fut>(
    mut attempt: F,
) -> Result<(), PostgresError>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), sqlx::Error>>,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match attempt().await {
            Ok(()) => return Ok(()),
            Err(sqlx::Error::Database(error))
                if attempts < MAX_ATTEMPTS && is_concurrent_catalog_ddl(error.as_ref()) =>
            {
                tokio::time::sleep(BACKOFF_STEP * attempts).await;
            }
            Err(error) => return Err(PostgresError::Db(error)),
        }
    }
}
