use std::time::{Duration, Instant};

use sqlx::PgPool;
use uuid::Uuid;

pub async fn staged_keys(pool: &PgPool, key: Uuid) -> i64 {
    let (count,): (i64,) = sqlx::query_as("SELECT count(*) FROM staged_impact WHERE key = $1")
        .bind(key.to_string())
        .fetch_one(pool)
        .await
        .expect("count staged impacts");
    count
}

pub async fn user_row(pool: &PgPool, user_id: Uuid) -> Option<(String, String)> {
    sqlx::query_as("SELECT email, xmin::text FROM known_users WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .expect("read the known_users row")
}

pub async fn service_account_name(pool: &PgPool, service_account_id: Uuid) -> Option<String> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT name FROM known_service_accounts WHERE service_account_id = $1")
            .bind(service_account_id)
            .fetch_optional(pool)
            .await
            .expect("read the known_service_accounts row");
    row.map(|(name,)| name)
}

pub async fn members(pool: &PgPool, group_id: Uuid) -> Vec<Uuid> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM known_user_group WHERE group_id = $1 ORDER BY user_id")
            .bind(group_id)
            .fetch_all(pool)
            .await
            .expect("read the memberships");
    rows.into_iter().map(|(id,)| id).collect()
}

pub async fn wait_for(label: &str, mut ready: impl AsyncFnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if ready().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {label}");
}
