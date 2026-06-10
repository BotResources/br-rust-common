//! Shared helpers for the scope-registration e2e suite.
//!
//! These tests drive the full `load → judge → save → dispatch` pipeline against
//! a **real** Postgres and a **real** NATS JetStream — no infra mocks. Gating:
//! `#[ignore]` by default, opted into with `--ignored`, and requiring **both**
//! `TEST_DATABASE_URL` and `NATS_URL`. Run single-threaded
//! (`--test-threads=1`); each test uses a dedicated per-test Postgres database
//! and unique role + durable-consumer names so re-runs never collide.
//!
//! The Postgres bootstrap mirrors production exactly (the `br-util-postgres`
//! e2e convention): admin (superuser) → owner role (`LOGIN CREATEROLE
//! NOSUPERUSER`, CNPG's `<svc>_owner`) → the owner runs migrations and creates
//! the app role → the app role gets least-privilege grants. The pipeline runs
//! through the **app pool**, so the e2e proves the runtime role can do exactly
//! what it needs and no more.
//!
//! Each e2e binary includes this module and uses a subset, so `dead_code` is
//! expected and silenced (the standard shared-helper pattern).
#![allow(dead_code)]

use std::sync::Arc;

use br_core_integration::NatsIntegrationPublisher;
use br_core_integration::{Actor, IntegrationCommand, MessageMetadata, UserId};
use br_core_scope::{
    DeclareServiceScopes, ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey, ServiceManifest,
};
use br_identity_app::{
    ConfirmationPublisher, ScopeDeclarationPipeline, ScopeRegistryRepository, migrate,
};
use chrono::Utc;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::str::FromStr;
use uuid::Uuid;

pub const APP_PW: &str = "identity_app_pw_e2e_only";

// ─── gating ────────────────────────────────────────────

pub fn test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

pub fn nats_url() -> Option<String> {
    std::env::var("NATS_URL").ok()
}

/// Both prerequisites must be present, else the test early-returns. The skip is
/// made **visible** (a silent early-return reads as a spurious pass): print why
/// before bailing so a run without the env vars is unmistakable.
pub fn prerequisites() -> Option<(String, String)> {
    match (test_db_url(), nats_url()) {
        (Some(db), Some(nats)) => Some((db, nats)),
        _ => {
            eprintln!("skipping: TEST_DATABASE_URL/NATS_URL unset");
            None
        }
    }
}

/// Unique per-test suffix (full v7 uuid simple), short enough for a role name
/// (≤63 bytes) and safe as an identifier.
pub fn unique_suffix() -> String {
    Uuid::now_v7().simple().to_string()[..24].to_string()
}

// ─── Postgres bootstrap (mirrors br-util-postgres e2e) ──

/// A bootstrapped Postgres environment for one test: a **dedicated per-test
/// database** (full isolation — the migration uses fixed table names, so two
/// tests cannot share the `public` schema without clashing on
/// `_sqlx_migrations` ownership), the admin pool (cleanup), the owner pool (ran
/// migrations + grants), the app pool (runtime, the pipeline uses it), and the
/// names to drop on teardown.
pub struct PgEnv {
    pub admin: PgPool,
    pub owner_pool: PgPool,
    pub app_pool: PgPool,
    pub owner: String,
    pub app_role: String,
    db_name: String,
    /// The admin URL aimed at the per-test database, for opening fresh pools.
    db_url: String,
}

impl PgEnv {
    /// Bootstrap: admin creates a fresh per-test database → owner (CREATEROLE
    /// NOSUPERUSER) owns it → migrate as owner → create app role → grant
    /// least-privilege on the registry tables → open the app pool. The pipeline
    /// runs through `app_pool`.
    pub async fn bootstrap(admin_url: &str) -> Self {
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(admin_url)
            .await
            .expect("connect as admin");

        // Per-test owner role and database. The owner OWNS the database, so it
        // gets CREATE on its public schema implicitly (mirrors CNPG's
        // `<svc>_owner` owning its `Database`).
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

        // Apply the crate's migrations as the owner (explicit invocation).
        migrate(&owner_pool).await.expect("migrate scope_registry");

        // Create the runtime app role and grant it least-privilege access.
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

    /// A fresh app-role pool against the per-test database (for the
    /// least-privilege probe, which uses a separate connection).
    pub async fn app_pool(&self) -> PgPool {
        open_pool_as(&self.db_url, &self.app_role, APP_PW)
            .await
            .expect("connect as app role")
    }

    /// Drop everything: close pools, drop the per-test database, then drop the
    /// app role and the owner (the owner owned the database).
    pub async fn teardown(self) {
        self.app_pool.close().await;
        self.owner_pool.close().await;
        // The database must be dropped before its owner role.
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

/// Replace the database path of a Postgres URL with `db_name`, preserving auth,
/// host, port, and query params.
fn swap_database(url: &str, db_name: &str) -> String {
    let mut parsed = url::Url::parse(url).expect("admin url parses");
    parsed.set_path(&format!("/{db_name}"));
    parsed.to_string()
}

async fn open_pool_as(url: &str, role: &str, password: &str) -> Result<PgPool, sqlx::Error> {
    let opts = PgConnectOptions::from_str(url)
        .expect("parse url")
        .username(role)
        .password(password);
    PgPoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
}

async fn cleanup_role(admin: &PgPool, role: &str) {
    let _ = sqlx::query(&format!("DROP OWNED BY \"{role}\" CASCADE"))
        .execute(admin)
        .await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
        .execute(admin)
        .await;
}

// ─── NATS bootstrap ────────────────────────────────────

/// The declare command subject for the slice.
pub const DECLARE_SUBJECT: &str = "identity.cmd.service_scope.declare.v1";
pub const ACCEPTED_SUBJECT: &str = "identity.evt.service_scope.accepted.v1";
pub const REJECTED_SUBJECT: &str = "identity.evt.service_scope.rejected.v1";

/// A real NATS JetStream environment for one test: the context, the
/// pre-declared command stream + durable consumer (the consumer binds them by
/// name — the lib never creates them), and a confirmations stream capturing the
/// `identity.evt.service_scope.>` replies so a test can read them back.
///
/// The subjects are the **production fixed subjects**, so the streams capturing
/// them must use **fixed names** too (two streams cannot both capture the same
/// subject). Test isolation comes from (a) `--test-threads=1`, (b) a
/// delete-then-create at the start of every test (so a crashed prior run cannot
/// leak captured messages), (c) a per-test unique durable consumer name, and (d)
/// every confirmation read being filtered by the test's unique `correlation_id`.
pub struct NatsEnv {
    pub js: async_nats::jetstream::Context,
    pub cmd_stream: String,
    pub evt_stream: String,
    pub durable: String,
}

impl NatsEnv {
    /// Pre-declare (as the operator/Helm would) the command stream capturing the
    /// declare subject, a durable consumer on it, and a confirmations stream
    /// capturing the accepted/rejected replies. The streams use fixed names and
    /// are dropped clean first; the durable name is unique per test.
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

        // Pre-declare the durable consumer the wrapper binds by name.
        let stream = js.get_stream(&cmd_stream).await.expect("get cmd stream");
        stream
            .create_consumer(async_nats::jetstream::consumer::pull::Config {
                durable_name: Some(durable.clone()),
                filter_subject: DECLARE_SUBJECT.to_string(),
                ack_policy: async_nats::jetstream::consumer::AckPolicy::Explicit,
                // Short ack-wait so a (non-)redelivery is observable fast.
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

    /// Await the confirmation correlated to `correlation_id` on the confirmations
    /// stream, within `timeout`. Returns the subject it arrived on and its JSON
    /// body. Reads with an ephemeral consumer over `identity.evt.service_scope.>`
    /// and filters on the envelope's `metadata.correlation_id` — exactly how a
    /// declarant's `CorrelatedAwaiter` would isolate its own reply.
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
                // Ephemeral (no durable_name): a per-read cursor, not infra.
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
            // A confirmation for another test (shared subject): ack and skip.
            message.ack().await.ok();
        }
    }

    /// Count the confirmations correlated to `correlation_id` on the
    /// confirmations stream, observing for `window`. Used to prove there is
    /// **exactly one** confirmation (no redelivery loop): a nak/term-without-reply
    /// would have produced a second one after the consumer's ack-wait. Reads from
    /// the stream start each call (a fresh ephemeral cursor), so it counts the
    /// total emitted, not just new arrivals.
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
                // Timed out waiting for the next message → observation window
                // elapsed with no further confirmation.
                _ => break,
            }
        }
        count
    }
}

/// A confirmation read back from the bus: its subject + JSON body.
pub struct Confirmation {
    pub subject: String,
    pub body: serde_json::Value,
}

impl Confirmation {
    /// Whether this is an `accepted` confirmation (by subject).
    pub fn is_accepted(&self) -> bool {
        self.subject == ACCEPTED_SUBJECT
    }
    /// Whether this is a `rejected` confirmation (by subject).
    pub fn is_rejected(&self) -> bool {
        self.subject == REJECTED_SUBJECT
    }
    /// The envelope's `causation_id`.
    pub fn causation_id(&self) -> Option<&str> {
        self.body["metadata"]["causation_id"].as_str()
    }
    /// The `service` echoed in the payload.
    pub fn service(&self) -> Option<&str> {
        self.body["payload"]["service"].as_str()
    }
    /// The rejection `reason` code, if this is a rejected confirmation.
    pub fn reason_code(&self) -> Option<&str> {
        self.body["payload"]["reason"]["reason"].as_str()
    }
    /// The `owner` carried by a `ScopeOwnedByAnotherService` rejection reason.
    pub fn reason_owner(&self) -> Option<&str> {
        self.body["payload"]["reason"]["owner"].as_str()
    }
}

// ─── command + payload builders ────────────────────────

/// A validated declare command with a known correlation + actor.
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

/// A declare command from raw JSON (the receiver path — lets a test inject a
/// structurally well-formed but invalid declaration, e.g. a prefix mismatch).
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
    let metadata = MessageMetadata::new(Actor::Human(UserId::from(actor)), correlation_id);
    IntegrationCommand::new(
        Uuid::now_v7(),
        "service_scope.declare",
        1,
        Utc::now(),
        metadata,
        payload,
    )
}

// ─── pipeline assembly ─────────────────────────────────

/// Assemble the pipeline against the app pool + a real JetStream publisher.
pub fn pipeline(
    pg: &PgEnv,
    js: async_nats::jetstream::Context,
) -> Arc<ScopeDeclarationPipeline<NatsIntegrationPublisher>> {
    let repository = ScopeRegistryRepository::new(pg.app_pool.clone());
    let publisher = Arc::new(NatsIntegrationPublisher::new(js));
    let confirmations = ConfirmationPublisher::new(publisher);
    Arc::new(ScopeDeclarationPipeline::new(repository, confirmations))
}
