#![allow(dead_code)]

use std::sync::Arc;

use br_core_integration::NatsIntegrationPublisher;
use br_core_integration::{Actor, EventMetadata, IntegrationCommand, UserId};
use br_core_scope::{
    DeclareServiceScopes, ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest,
};
use br_identity_app::{
    ConfirmationPublisher, ScopeDeclarationPipeline, ScopeRegistryRepository, migrate,
};
pub use br_test_support::test_db_url;
use br_test_support::{cleanup_role, open_pool_as, unique_suffix};
use chrono::Utc;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;

pub const APP_PW: &str = "identity_app_pw_e2e_only";

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

pub fn prerequisites() -> Option<(String, String)> {
    match (test_db_url(), nats_url()) {
        (Some(db), Some(nats)) => Some((db, nats)),
        _ => {
            eprintln!("skipping: TEST_DATABASE_URL/NATS_URL unset");
            None
        }
    }
}

pub struct PgEnv {
    pub admin: PgPool,
    pub owner_pool: PgPool,
    pub app_pool: PgPool,
    pub owner: String,
    pub app_role: String,
    db_name: String,
    db_url: String,
}

impl PgEnv {
    pub async fn bootstrap(admin_url: &str) -> Self {
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(admin_url)
            .await
            .expect("connect as admin");

        let owner = format!("br_test_owner_{}", unique_suffix());
        sqlx::query(&format!(
            "CREATE ROLE \"{owner}\" LOGIN CREATEROLE NOSUPERUSER PASSWORD 'owner_pw_e2e_only'"
        ))
        .execute(&admin)
        .await
        .expect("create owner role");

        let db_name = format!("br_test_db_{}", unique_suffix());
        sqlx::query(&format!("CREATE DATABASE \"{db_name}\" OWNER \"{owner}\""))
            .execute(&admin)
            .await
            .expect("create per-test database");

        let db_url = swap_database(admin_url, &db_name);

        let owner_opts = PgConnectOptions::from_str(&db_url)
            .expect("parse url")
            .username(&owner)
            .password("owner_pw_e2e_only");
        let owner_pool = PgPoolOptions::new()
            .max_connections(2)
            .connect_with(owner_opts)
            .await
            .expect("connect as owner");

        migrate(&owner_pool).await.expect("migrate scope_registry");

        let app_role = format!("br_test_app_{}", unique_suffix());
        br_util_postgres::ensure_app_role(&owner_pool, &app_role, APP_PW)
            .await
            .expect("ensure app role");
        br_util_postgres::grant_app_access(&owner_pool, &app_role)
            .await
            .expect("grant app access");

        let app_pool = open_pool_as(&db_url, &app_role, APP_PW)
            .await
            .expect("connect as app role");

        Self {
            admin,
            owner_pool,
            app_pool,
            owner,
            app_role,
            db_name,
            db_url,
        }
    }

    pub async fn app_pool(&self) -> PgPool {
        open_pool_as(&self.db_url, &self.app_role, APP_PW)
            .await
            .expect("connect as app role")
    }

    pub async fn teardown(self) {
        self.app_pool.close().await;
        self.owner_pool.close().await;
        let _ = sqlx::query(&format!(
            "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
            self.db_name
        ))
        .execute(&self.admin)
        .await;
        cleanup_role(&self.admin, &self.app_role).await;
        cleanup_role(&self.admin, &self.owner).await;
        self.admin.close().await;
    }
}

fn swap_database(url: &str, db_name: &str) -> String {
    let mut parsed = url::Url::parse(url).expect("admin url parses");
    parsed.set_path(&format!("/{db_name}"));
    parsed.to_string()
}

pub const DECLARE_SUBJECT: &str = "identity.cmd.service_scope.declare.v1";
pub const ACCEPTED_SUBJECT: &str = "identity.evt.service_scope.accepted.v1";
pub const REJECTED_SUBJECT: &str = "identity.evt.service_scope.rejected.v1";

pub struct NatsEnv {
    pub js: async_nats::jetstream::Context,
    pub cmd_stream: String,
    pub evt_stream: String,
    pub durable: String,
}

impl NatsEnv {
    pub async fn bootstrap(url: &str) -> Self {
        let client = async_nats::connect(url).await.expect("connect to NATS");
        let js = async_nats::jetstream::new(client);

        let cmd_stream = "IDENTITY_CMD_E2E".to_string();
        let evt_stream = "IDENTITY_EVT_E2E".to_string();
        let durable = format!("declare_worker_{}", unique_suffix());

        let _ = js.delete_stream(&cmd_stream).await;
        let _ = js.delete_stream(&evt_stream).await;

        js.create_stream(async_nats::jetstream::stream::Config {
            name: cmd_stream.clone(),
            subjects: vec![DECLARE_SUBJECT.to_string()],
            ..Default::default()
        })
        .await
        .expect("create command stream");

        js.create_stream(async_nats::jetstream::stream::Config {
            name: evt_stream.clone(),
            subjects: vec!["identity.evt.service_scope.>".to_string()],
            ..Default::default()
        })
        .await
        .expect("create confirmations stream");

        let stream = js.get_stream(&cmd_stream).await.expect("get cmd stream");
        stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some(durable.clone()),
                filter_subject: DECLARE_SUBJECT.to_string(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ack_wait: std::time::Duration::from_secs(2),
                ..Default::default()
            })
            .await
            .expect("create durable consumer");

        Self {
            js,
            cmd_stream,
            evt_stream,
            durable,
        }
    }

    pub async fn teardown(self) {
        let _ = self.js.delete_stream(&self.cmd_stream).await;
        let _ = self.js.delete_stream(&self.evt_stream).await;
    }

    pub async fn await_confirmation(
        &self,
        correlation_id: Uuid,
        timeout: std::time::Duration,
    ) -> Confirmation {
        use futures_util::StreamExt;

        let stream = self
            .js
            .get_stream(&self.evt_stream)
            .await
            .expect("evt stream");
        let consumer = stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            })
            .await
            .expect("create ephemeral reader");

        let deadline = tokio::time::Instant::now() + timeout;
        let mut messages = consumer.messages().await.expect("messages stream");
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let next = tokio::time::timeout(remaining, messages.next()).await;
            let message = next
                .expect("a confirmation arrived before the deadline")
                .expect("message stream open")
                .expect("message");
            let body: serde_json::Value =
                serde_json::from_slice(&message.payload).expect("confirmation is json");
            let arrived = body["metadata"]["correlation_id"].as_str();
            if arrived == Some(correlation_id.to_string().as_str()) {
                let subject = message.subject.as_str().to_string();
                message.ack().await.expect("ack the read confirmation");
                return Confirmation { subject, body };
            }
            message.ack().await.ok();
        }
    }

    pub async fn count_confirmations(
        &self,
        correlation_id: Uuid,
        window: std::time::Duration,
    ) -> usize {
        use futures_util::StreamExt;

        let stream = self
            .js
            .get_stream(&self.evt_stream)
            .await
            .expect("evt stream");
        let consumer = stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            })
            .await
            .expect("create ephemeral reader");

        let deadline = tokio::time::Instant::now() + window;
        let mut messages = consumer.messages().await.expect("messages stream");
        let mut count = 0;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, messages.next()).await {
                Ok(Some(Ok(message))) => {
                    let body: serde_json::Value =
                        serde_json::from_slice(&message.payload).unwrap_or_default();
                    if body["metadata"]["correlation_id"].as_str()
                        == Some(correlation_id.to_string().as_str())
                    {
                        count += 1;
                    }
                    message.ack().await.ok();
                }
                _ => break,
            }
        }
        count
    }
}

pub struct Confirmation {
    pub subject: String,
    pub body: serde_json::Value,
}

impl Confirmation {
    pub fn is_accepted(&self) -> bool {
        self.subject == ACCEPTED_SUBJECT
    }
    pub fn is_rejected(&self) -> bool {
        self.subject == REJECTED_SUBJECT
    }
    pub fn causation_id(&self) -> Option<&str> {
        self.body["metadata"]["causation_id"].as_str()
    }
    pub fn service(&self) -> Option<&str> {
        self.body["payload"]["service"].as_str()
    }
    pub fn reason_code(&self) -> Option<&str> {
        self.body["payload"]["reason"]["reason"].as_str()
    }
    pub fn reason_owner(&self) -> Option<&str> {
        self.body["payload"]["reason"]["owner"].as_str()
    }
}

pub fn declare_command(
    service: &str,
    scopes: &[(&str, bool)],
    correlation_id: Uuid,
    actor: Uuid,
) -> IntegrationCommand<DeclareServiceScopes> {
    let specs = scopes
        .iter()
        .map(|(k, platform_only)| {
            ScopeSpec::new(
                ScopeKey::new(*k).unwrap(),
                format!("scope.{k}.label"),
                format!("scope.{k}.desc"),
                *platform_only,
            )
        })
        .collect();
    let manifest = ServiceManifest::new(
        ServiceKey::new(service).unwrap(),
        format!("service.{service}.label"),
        format!("service.{service}.desc"),
    );
    let payload = DeclareServiceScopes::new(ScopeDeclaration::new(manifest, specs).unwrap());
    integration_command(payload, correlation_id, actor)
}

pub fn declare_command_from_json(
    json: &str,
    correlation_id: Uuid,
    actor: Uuid,
) -> IntegrationCommand<DeclareServiceScopes> {
    let payload: DeclareServiceScopes = serde_json::from_str(json).expect("valid declare json");
    integration_command(payload, correlation_id, actor)
}

fn integration_command(
    payload: DeclareServiceScopes,
    correlation_id: Uuid,
    actor: Uuid,
) -> IntegrationCommand<DeclareServiceScopes> {
    let metadata = EventMetadata::new(Actor::Human(UserId::from(actor)), correlation_id);
    IntegrationCommand::new(
        Uuid::now_v7(),
        "service_scope.declare",
        1,
        Utc::now(),
        metadata,
        payload,
    )
}

pub fn pipeline(
    pg: &PgEnv,
    js: async_nats::jetstream::Context,
) -> Arc<ScopeDeclarationPipeline<NatsIntegrationPublisher>> {
    let repository = ScopeRegistryRepository::new(pg.app_pool.clone());
    let publisher = Arc::new(NatsIntegrationPublisher::new(js));
    let confirmations = ConfirmationPublisher::new(publisher);
    Arc::new(ScopeDeclarationPipeline::new(repository, confirmations))
}
