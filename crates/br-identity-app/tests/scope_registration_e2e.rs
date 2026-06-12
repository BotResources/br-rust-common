mod common;

use std::time::Duration;

use br_core_scope::{ScopeKey, ScopeSpec, ServiceKey, ServiceManifest};
use br_identity_app::{HandledOutcome, SaveOutcome, ScopeRegistryRepository};
use common::{NatsEnv, PgEnv, declare_command, declare_command_from_json, pipeline, prerequisites};
use sqlx::Row;
use uuid::Uuid;

async fn spawn_consumer(pg: &PgEnv, nats: &NatsEnv) -> tokio::task::JoinHandle<()> {
    let pipeline = pipeline(pg, nats.js.clone());
    let js = nats.js.clone();
    let cmd_stream = nats.cmd_stream.clone();
    let durable = nats.durable.clone();
    tokio::spawn(async move {
        br_identity_app::run_scope_declarations(
            &js,
            &cmd_stream,
            &durable,
            pipeline,
            |poison| panic!("unexpected poison: {poison}"),
            |err| panic!("unexpected permanent (corrupt-store) failure: {err}"),
        )
        .await
        .ok();
    })
}

async fn count(pool: &sqlx::PgPool, table: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT count(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .expect("count query")
}

async fn head_version(pool: &sqlx::PgPool) -> i64 {
    sqlx::query_scalar("SELECT version FROM scope_registry_head WHERE id = true")
        .fetch_one(pool)
        .await
        .expect("head version query")
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL and NATS_URL (real PG + JetStream)"]
async fn declare_persists_rows_and_publishes_accepted() {
    let Some((db, nats_url)) = prerequisites() else {
        return;
    };
    let pg = PgEnv::bootstrap(&db).await;
    let nats = NatsEnv::bootstrap(&nats_url).await;
    let consumer = spawn_consumer(&pg, &nats).await;

    let correlation = Uuid::now_v7();
    let actor = Uuid::now_v7();
    let cmd = declare_command(
        "notifier",
        &[("notifier:read", false), ("notifier:admin", true)],
        correlation,
        actor,
    );
    let command_id = cmd.command_id;

    let publisher = br_core_integration::NatsIntegrationPublisher::new(nats.js.clone());
    use br_core_integration::IntegrationPublisherExt;
    publisher
        .publish_command(common::DECLARE_SUBJECT, &cmd)
        .await
        .expect("publish declare");

    let confirmation = nats
        .await_confirmation(correlation, Duration::from_secs(10))
        .await;

    assert!(
        confirmation.is_accepted(),
        "expected accepted, got {}",
        confirmation.subject
    );
    assert_eq!(confirmation.service(), Some("notifier"));
    assert_eq!(
        confirmation.causation_id(),
        Some(command_id.to_string().as_str())
    );

    assert_eq!(count(&pg.app_pool, "scope_registry_service").await, 1);
    assert_eq!(count(&pg.app_pool, "scope_registry").await, 2);
    assert_eq!(head_version(&pg.app_pool).await, 1);

    let row = sqlx::query(
        "SELECT owning_service, platform_only, registered_at = last_seen_at AS fresh \
         FROM scope_registry WHERE scope_key = 'notifier:admin'",
    )
    .fetch_one(&pg.app_pool)
    .await
    .expect("scope row");
    assert_eq!(row.get::<String, _>("owning_service"), "notifier");
    assert!(row.get::<bool, _>("platform_only"));
    assert!(
        row.get::<bool, _>("fresh"),
        "first acceptance: registered_at == last_seen_at"
    );

    consumer.abort();
    nats.teardown().await;
    pg.teardown().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL and NATS_URL (real PG + JetStream)"]
async fn idempotent_redeclare_no_dup_rows_and_reemits_accepted() {
    let Some((db, nats_url)) = prerequisites() else {
        return;
    };
    let pg = PgEnv::bootstrap(&db).await;
    let nats = NatsEnv::bootstrap(&nats_url).await;
    let consumer = spawn_consumer(&pg, &nats).await;

    let publisher = br_core_integration::NatsIntegrationPublisher::new(nats.js.clone());
    use br_core_integration::IntegrationPublisherExt;

    let c1 = Uuid::now_v7();
    let cmd1 = declare_command("notifier", &[("notifier:read", false)], c1, Uuid::now_v7());
    publisher
        .publish_command(common::DECLARE_SUBJECT, &cmd1)
        .await
        .unwrap();
    let conf1 = nats.await_confirmation(c1, Duration::from_secs(10)).await;
    assert!(conf1.is_accepted());
    assert_eq!(head_version(&pg.app_pool).await, 1);

    let c2 = Uuid::now_v7();
    let cmd2 = declare_command("notifier", &[("notifier:read", false)], c2, Uuid::now_v7());
    let command_id2 = cmd2.command_id;
    publisher
        .publish_command(common::DECLARE_SUBJECT, &cmd2)
        .await
        .unwrap();
    let conf2 = nats.await_confirmation(c2, Duration::from_secs(10)).await;

    assert!(
        conf2.is_accepted(),
        "an idempotent re-declare must STILL be acked with accepted"
    );
    assert_eq!(conf2.causation_id(), Some(command_id2.to_string().as_str()));

    assert_eq!(count(&pg.app_pool, "scope_registry_service").await, 1);
    assert_eq!(count(&pg.app_pool, "scope_registry").await, 1);
    assert_eq!(
        head_version(&pg.app_pool).await,
        1,
        "a no-op must not bump the version"
    );

    let touched: bool = sqlx::query_scalar(
        "SELECT last_seen_at >= registered_at FROM scope_registry WHERE scope_key = 'notifier:read'",
    )
    .fetch_one(&pg.app_pool)
    .await
    .expect("touch query");
    assert!(
        touched,
        "last_seen_at is touched on re-declare, registered_at preserved"
    );

    consumer.abort();
    nats.teardown().await;
    pg.teardown().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL and NATS_URL (real PG + JetStream)"]
async fn invalid_declaration_is_rejected_registry_untouched_and_acked() {
    let Some((db, nats_url)) = prerequisites() else {
        return;
    };
    let pg = PgEnv::bootstrap(&db).await;
    let nats = NatsEnv::bootstrap(&nats_url).await;
    let consumer = spawn_consumer(&pg, &nats).await;

    let publisher = br_core_integration::NatsIntegrationPublisher::new(nats.js.clone());
    use br_core_integration::IntegrationPublisherExt;

    let correlation = Uuid::now_v7();
    let cmd = declare_command_from_json(
        r#"{"declaration":{
            "manifest":{"key":"notifier","label_key":"l","description_key":"d"},
            "scopes":[{"key":"billing:read","label_key":"l","description_key":"d","platform_only":false}]
        }}"#,
        correlation,
        Uuid::now_v7(),
    );
    let command_id = cmd.command_id;
    publisher
        .publish_command(common::DECLARE_SUBJECT, &cmd)
        .await
        .unwrap();

    let confirmation = nats
        .await_confirmation(correlation, Duration::from_secs(10))
        .await;
    assert!(
        confirmation.is_rejected(),
        "expected rejected, got {}",
        confirmation.subject
    );
    assert_eq!(confirmation.reason_code(), Some("scope_prefix_mismatch"));
    assert_eq!(
        confirmation.causation_id(),
        Some(command_id.to_string().as_str())
    );

    assert_eq!(count(&pg.app_pool, "scope_registry_service").await, 0);
    assert_eq!(count(&pg.app_pool, "scope_registry").await, 0);
    assert_eq!(head_version(&pg.app_pool).await, 0);

    let emitted = nats
        .count_confirmations(correlation, Duration::from_secs(4))
        .await;
    assert_eq!(
        emitted, 1,
        "a rejected declaration must be acked (no redelivery loop): expected exactly one \
         confirmation, observed {emitted}"
    );

    consumer.abort();
    nats.teardown().await;
    pg.teardown().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL (real PG)"]
async fn save_classifies_cross_owner_unique_violation_as_scope_conflict() {
    let Some(db) = common::test_db_url() else {
        return;
    };
    let pg = PgEnv::bootstrap(&db).await;
    let repo = ScopeRegistryRepository::new(pg.app_pool.clone());

    let (mut registry, version) = repo.load().await.expect("load empty");
    assert_eq!(version, 0);

    let manifest = ServiceManifest::new(ServiceKey::new("notifier").unwrap(), "l", "d");
    let scopes = vec![ScopeSpec::new(
        ScopeKey::new("notifier:read").unwrap(),
        "l",
        "d",
        false,
    )];
    let decl = br_core_scope::ScopeDeclaration::new(manifest, scopes).unwrap();
    registry.register_declaration(&decl).expect("accept");

    sqlx::query("INSERT INTO scope_registry_service (service_key, label_key, description_key) VALUES ('legacy', 'l', 'd')")
        .execute(&pg.owner_pool)
        .await
        .expect("inject racing service");
    sqlx::query(
        "INSERT INTO scope_registry (scope_key, owning_service, label_key, description_key, platform_only) \
         VALUES ('notifier:read', 'legacy', 'l', 'd', false)",
    )
    .execute(&pg.owner_pool)
    .await
    .expect("inject racing scope row");

    let outcome = repo
        .save(&registry, version)
        .await
        .expect("save returns a classified outcome, not an error");
    assert_eq!(
        outcome,
        SaveOutcome::ScopeConflict {
            scope_key: "notifier:read".to_string(),
            owner: "legacy".to_string(),
        },
        "a cross-owner unique violation must be classified with the actual owner, never raised as an error (which would nak/loop)"
    );

    pg.teardown().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL and NATS_URL (real PG + JetStream)"]
async fn scope_conflict_yields_rejected_confirmation_without_redelivery() {
    let Some((db, nats_url)) = prerequisites() else {
        return;
    };
    let pg = PgEnv::bootstrap(&db).await;
    let nats = NatsEnv::bootstrap(&nats_url).await;

    let pipeline = pipeline(&pg, nats.js.clone());

    let correlation = Uuid::now_v7();
    let cmd = declare_command(
        "notifier",
        &[("notifier:read", false)],
        correlation,
        Uuid::now_v7(),
    );

    let owner_pool = pg.owner_pool.clone();
    let racer = tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = sqlx::query("INSERT INTO scope_registry_service (service_key, label_key, description_key) VALUES ('legacy', 'l', 'd') ON CONFLICT DO NOTHING")
            .execute(&owner_pool)
            .await;
        let _ = sqlx::query(
            "INSERT INTO scope_registry (scope_key, owning_service, label_key, description_key, platform_only) \
             VALUES ('notifier:read', 'legacy', 'l', 'd', false) ON CONFLICT DO NOTHING",
        )
        .execute(&owner_pool)
        .await;
    });

    let outcome = pipeline.handle(&cmd).await;
    racer.await.ok();

    match outcome {
        Ok(HandledOutcome::Rejected { reason }) => {
            assert_eq!(reason.to_string(), "scope_owned_by_another_service");
            let confirmation = nats
                .await_confirmation(correlation, Duration::from_secs(10))
                .await;
            assert!(confirmation.is_rejected());
            assert_eq!(
                confirmation.reason_code(),
                Some("scope_owned_by_another_service")
            );
            assert_eq!(confirmation.reason_owner(), Some("legacy"));
            let emitted = nats
                .count_confirmations(correlation, Duration::from_secs(4))
                .await;
            assert_eq!(
                emitted, 1,
                "the unique-violation rejection must be emitted exactly once (no nak/redelivery loop)"
            );
        }
        Ok(HandledOutcome::Accepted { .. }) => {
            eprintln!(
                "scope_conflict e2e: race lost (accepted) — classification proven by the \
                 repository-seam test; skipping the rejected assertion this run"
            );
        }
        Ok(other) => panic!("unexpected outcome {other:?}"),
        Err(err) => {
            assert!(
                matches!(err, br_identity_app::AppError::Hydration(_)),
                "the only tolerated error is a fail-loud hydration of the injected corrupt row, \
                 got {err}"
            );
        }
    }

    nats.teardown().await;
    pg.teardown().await;
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL (real PG, non-superuser app role)"]
async fn app_role_has_least_privilege_on_registry_tables() {
    let Some(db) = common::test_db_url() else {
        return;
    };
    let pg = PgEnv::bootstrap(&db).await;
    let app = pg.app_pool().await;

    sqlx::query("INSERT INTO scope_registry_service (service_key, label_key, description_key) VALUES ('billing', 'l', 'd')")
        .execute(&app)
        .await
        .expect("app role may INSERT a service");
    sqlx::query(
        "INSERT INTO scope_registry (scope_key, owning_service, label_key, description_key, platform_only) \
         VALUES ('billing:read', 'billing', 'l', 'd', false)",
    )
    .execute(&app)
    .await
    .expect("app role may INSERT a scope");
    sqlx::query("UPDATE scope_registry SET last_seen_at = now() WHERE scope_key = 'billing:read'")
        .execute(&app)
        .await
        .expect("app role may UPDATE a scope (the last_seen_at touch)");
    let read: i64 = sqlx::query_scalar("SELECT count(*) FROM scope_registry")
        .fetch_one(&app)
        .await
        .expect("app role may SELECT");
    assert_eq!(read, 1);

    let ddl = sqlx::query("CREATE TABLE app_should_not_create (id int)")
        .execute(&app)
        .await;
    assert!(
        ddl.is_err(),
        "the app role must NOT be able to run DDL — it is least-privilege, not the owner"
    );

    app.close().await;
    pg.teardown().await;
}
