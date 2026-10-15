use std::str::FromStr;

use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

const DEFAULT_MAX_CONNECTIONS: u32 = 2;

async fn connect_as(
    url: &str,
    role: &str,
    password: &str,
    max_connections: u32,
) -> Result<PgPool, sqlx::Error> {
    let options = PgConnectOptions::from_str(url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(role)
        .password(password);
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
}

pub async fn open_pool_as(url: &str, role: &str, password: &str) -> Result<PgPool, sqlx::Error> {
    connect_as(url, role, password, DEFAULT_MAX_CONNECTIONS).await
}

pub async fn wide_caller_pool(
    url: &str,
    role: &str,
    password: &str,
    max_connections: u32,
) -> PgPool {
    connect_as(url, role, password, max_connections)
        .await
        .expect("connect as caller")
}
