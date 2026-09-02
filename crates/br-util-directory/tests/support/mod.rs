#![allow(dead_code)]

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use br_core_directory::{
    DIRECTORY_META_VERSION, DirectoryMeta, PublishedEntity, PublishedGroup,
    PublishedServiceAccount, PublishedUser,
};
use br_util_directory::{DirectoryPublisher, ForeignRef, Impact, ImpactStager};
use br_util_nats_fabric::{Fabric, KV_PUBLISHED_LANGUAGE};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{Connection, PgConnection, PgPool};
use uuid::Uuid;

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
    let nats = std::env::var("NATS_URL")
        .expect("NATS_URL and TEST_DATABASE_URL are required for this ignored suite");
    let database = br_test_support::test_db_url()
        .expect("NATS_URL and TEST_DATABASE_URL are required for this ignored suite");
    (nats, database)
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

pub struct RecordingStager {
    seen: Arc<Mutex<Vec<Impact>>>,
    fail: bool,
}

impl RecordingStager {
    pub fn accepting() -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            fail: false,
        }
    }

    pub fn failing() -> Self {
        Self {
            seen: Arc::new(Mutex::new(Vec::new())),
            fail: true,
        }
    }

    pub fn seen(&self) -> Arc<Mutex<Vec<Impact>>> {
        Arc::clone(&self.seen)
    }
}

#[async_trait::async_trait]
impl ImpactStager for RecordingStager {
    async fn stage_in(
        &self,
        conn: &mut sqlx::PgConnection,
        impacts: &[Impact],
    ) -> Result<(), br_util_directory::DirectoryError> {
        for impact in impacts {
            let foreign = foreign_ref(impact);
            sqlx::query("INSERT INTO staged_impact (namespace, key) VALUES ($1, $2)")
                .bind(foreign.namespace())
                .bind(foreign.key())
                .execute(&mut *conn)
                .await?;
        }
        self.seen
            .lock()
            .expect("stager record lock")
            .extend_from_slice(impacts);
        if self.fail {
            return Err(br_util_directory::DirectoryError::Persistence(
                sqlx::Error::RowNotFound,
            ));
        }
        Ok(())
    }
}

pub fn impacts_for(seen: &Arc<Mutex<Vec<Impact>>>, id: Uuid) -> usize {
    seen.lock()
        .expect("record lock")
        .iter()
        .filter(|impact| foreign_ref(impact).key() == id.to_string())
        .count()
}

pub fn foreign_ref(impact: &Impact) -> &ForeignRef {
    match impact {
        Impact::ForeignChanged { foreign } => foreign,
        other => panic!("unexpected impact variant: {other:?}"),
    }
}

pub fn manifest() -> DirectoryMeta {
    DirectoryMeta {
        version: DIRECTORY_META_VERSION,
        entities: vec![PublishedEntity::Users, PublishedEntity::Groups],
    }
}

pub fn manifest_with_service_accounts() -> DirectoryMeta {
    DirectoryMeta {
        version: DIRECTORY_META_VERSION,
        entities: vec![
            PublishedEntity::Users,
            PublishedEntity::Groups,
            PublishedEntity::ServiceAccounts,
        ],
    }
}

pub fn user(email: &str) -> PublishedUser {
    PublishedUser::new(
        email.to_string(),
        Some("Ada".to_string()),
        Some("Lovelace".to_string()),
        BTreeMap::new(),
    )
    .expect("a published user")
}

pub fn group(name: &str, members: &[Uuid]) -> PublishedGroup {
    PublishedGroup::new(name.to_string(), members.to_vec(), BTreeMap::new())
        .expect("a published group")
}

pub fn service_account(name: &str) -> PublishedServiceAccount {
    PublishedServiceAccount::new(name.to_string(), BTreeMap::new())
        .expect("a published service account")
}

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

pub async fn drop_published_user(fabric: &Fabric, user_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    publisher
        .retract_user(user_id)
        .await
        .expect("retract the published user");
}

pub async fn drop_published_group(fabric: &Fabric, group_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    publisher
        .retract_group(group_id)
        .await
        .expect("retract the published group");
}

pub async fn drop_published_service_account(fabric: &Fabric, service_account_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    publisher
        .retract_service_account(service_account_id)
        .await
        .expect("retract the published service account");
}
