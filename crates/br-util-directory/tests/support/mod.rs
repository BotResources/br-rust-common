use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use br_core_directory::{
    DIRECTORY_META_VERSION, DirectoryMeta, PublishedEntity, PublishedGroup, PublishedUser,
};
use br_util_directory::{DirectoryPublisher, ForeignRef, Impact, ImpactStager};
use br_util_nats_fabric::{Fabric, KV_PUBLISHED_LANGUAGE};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use uuid::Uuid;

pub static PROJECTION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

pub fn infra() -> Option<(String, String)> {
    Some((nats_url()?, br_test_support::test_db_url()?))
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

pub async fn members(pool: &PgPool, group_id: Uuid) -> Vec<Uuid> {
    let rows: Vec<(Uuid,)> =
        sqlx::query_as("SELECT user_id FROM known_user_group WHERE group_id = $1 ORDER BY user_id")
            .bind(group_id)
            .fetch_all(pool)
            .await
            .expect("read the memberships");
    rows.into_iter().map(|(id,)| id).collect()
}

pub async fn drop_published_user(fabric: &Fabric, user_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    let _ = publisher.retract_user(user_id).await;
}

pub async fn drop_published_group(fabric: &Fabric, group_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    let _ = publisher.retract_group(group_id).await;
}
