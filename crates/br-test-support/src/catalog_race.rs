use std::future::Future;
use std::time::Duration;

const ATTEMPTS: u32 = 20;
const BACKOFF: Duration = Duration::from_millis(25);

fn is_catalog_race(error: &sqlx::Error) -> bool {
    matches!(
        error,
        sqlx::Error::Database(db) if matches!(db.code().as_deref(), Some("XX000" | "23505" | "42710"))
    )
}

pub(crate) async fn retry_catalog_race<F, Fut>(mut attempt: F) -> Result<(), sqlx::Error>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<(), sqlx::Error>>,
{
    let mut attempts = 0;
    loop {
        attempts += 1;
        match attempt().await {
            Err(error) if attempts < ATTEMPTS && is_catalog_race(&error) => {
                tokio::time::sleep(BACKOFF).await;
            }
            other => return other,
        }
    }
}
