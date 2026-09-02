#![allow(dead_code, unused_imports)]

mod reads;
mod roster;
mod stager;

pub use reads::{members, service_account_name, staged_in, staged_keys, user_row, wait_for};
pub use roster::{
    drop_published_group, drop_published_service_account, drop_published_user, group, manifest,
    manifest_with_service_accounts, nameless_user, publish_until, publish_until_projected,
    service_account, user,
};
pub use stager::{RecordingStager, foreign_ref, impacts_for};

use std::str::FromStr;

use br_util_nats_fabric::{Fabric, KV_PUBLISHED_LANGUAGE};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};

const PROJECTION_ADVISORY_LOCK: i64 = 0x62_72_77_70_34_00_01;

pub struct ExclusiveProjection {
    connection: Option<PgConnection>,
}

impl ExclusiveProjection {
    pub async fn release(mut self) {
        if let Some(mut connection) = self.connection.take() {
            sqlx::query("SELECT pg_advisory_unlock($1)")
                .bind(PROJECTION_ADVISORY_LOCK)
                .execute(&mut connection)
                .await
                .expect("release the projection advisory lock");
            connection
                .close()
                .await
                .expect("close the advisory lock connection");
        }
    }
}

pub fn infra() -> (String, String) {
    (
        br_test_support::require_nats_url(),
        br_test_support::require_test_db_url(),
    )
}

pub async fn exclusive_projection(database_url: &str) -> ExclusiveProjection {
    let mut connection = PgConnection::connect(database_url)
        .await
        .expect("connect to TEST_DATABASE_URL");
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(PROJECTION_ADVISORY_LOCK)
        .execute(&mut connection)
        .await
        .expect("take the projection advisory lock");
    ExclusiveProjection {
        connection: Some(connection),
    }
}

pub async fn fabric(url: &str) -> Fabric {
    let client = async_nats::connect(url).await.expect("connect to NATS");
    let jetstream = async_nats::jetstream::new(client);
    if jetstream
        .get_key_value(KV_PUBLISHED_LANGUAGE)
        .await
        .is_err()
    {
        jetstream
            .create_key_value(async_nats::jetstream::kv::Config {
                bucket: KV_PUBLISHED_LANGUAGE.to_string(),
                ..Default::default()
            })
            .await
            .expect("create the published-language bucket");
    }
    Fabric::new(jetstream)
}

pub async fn isolated_pool(database_url: &str) -> PgPool {
    let schema = format!("wp4_{}", br_test_support::unique_suffix());
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await
        .expect("connect to TEST_DATABASE_URL");
    sqlx::query(&format!("CREATE SCHEMA \"{schema}\""))
        .execute(&admin)
        .await
        .expect("create the test schema");
    admin.close().await;

    let options = PgConnectOptions::from_str(database_url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .options([("search_path", schema.as_str())]);
    let pool = PgPoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await
        .expect("connect to the test schema");

    br_util_directory::migrate(&pool)
        .await
        .expect("run the known_* migrations");
    sqlx::query("CREATE TABLE staged_impact (namespace text NOT NULL, key text NOT NULL)")
        .execute(&pool)
        .await
        .expect("create the adopter impact table");
    pool
}
