use std::str::FromStr;
use std::sync::Arc;

use br_test_support::{
    cleanup_role, open_pool_as, require_test_db_url, setup_caller, unique_role_name,
    unique_table_name,
};
use br_util_postgres::{ensure_app_role, grant_app_access};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio::sync::Barrier;

const CALLER_PW: &str = "caller_pw_for_e2e_only";
const APP_PW: &str = "app_pw_for_e2e_only";
const PARALLEL_CALLS: usize = 16;
const ROUNDS: usize = 5;

async fn wide_caller_pool(url: &str, caller: &str) -> PgPool {
    let options = PgConnectOptions::from_str(url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(caller)
        .password(CALLER_PW);
    PgPoolOptions::new()
        .max_connections(PARALLEL_CALLS as u32)
        .connect_with(options)
        .await
        .expect("connect as caller")
}

async fn parallel_grants(pool: &PgPool, roles: &[String], calls: usize) -> Vec<Result<(), String>> {
    let barrier = Arc::new(Barrier::new(calls));
    let handles: Vec<_> = (0..calls)
        .map(|i| {
            let pool = pool.clone();
            let barrier = Arc::clone(&barrier);
            let role = roles[i % roles.len()].clone();
            tokio::spawn(async move {
                barrier.wait().await;
                grant_app_access(&pool, &role)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .collect();
    let mut results = Vec::with_capacity(calls);
    for handle in handles {
        results.push(handle.await.expect("task must not panic"));
    }
    results
}

async fn assert_round(url: &str, owner_pool: &PgPool, roles: &[String], label: &str, round: usize) {
    let results = parallel_grants(owner_pool, roles, PARALLEL_CALLS).await;
    let failures: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();
    assert!(
        failures.is_empty(),
        "{label} round {round}: {} of {PARALLEL_CALLS} parallel grants failed: {failures:?}",
        failures.len()
    );

    let table = unique_table_name();
    sqlx::query(&format!("CREATE TABLE \"{table}\" (id int)"))
        .execute(owner_pool)
        .await
        .expect("create table");
    sqlx::query(&format!("INSERT INTO \"{table}\" (id) VALUES (7)"))
        .execute(owner_pool)
        .await
        .expect("insert");
    let mut reads = Vec::new();
    for role in roles {
        let app_pool = open_pool_as(url, role, APP_PW)
            .await
            .expect("app login after grants");
        reads.push(
            sqlx::query_scalar::<_, i32>(&format!("SELECT id FROM \"{table}\""))
                .fetch_one(&app_pool)
                .await,
        );
        app_pool.close().await;
    }
    sqlx::query(&format!("DROP TABLE \"{table}\""))
        .execute(owner_pool)
        .await
        .expect("drop table");
    for read in reads {
        assert_eq!(
            read.expect("app role must read a table created after the grants"),
            7
        );
    }
}

async fn run_rounds(label: &str, role_count: usize) {
    let url = require_test_db_url();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect as admin");
    let (setup_pool, owner) = setup_caller(&admin, &url).await;
    setup_pool.close().await;
    let owner_pool = wide_caller_pool(&url, &owner).await;

    for round in 0..ROUNDS {
        let mut roles = Vec::new();
        for _ in 0..role_count {
            let role = unique_role_name();
            ensure_app_role(&owner_pool, &role, APP_PW)
                .await
                .expect("ensure app role");
            roles.push(role);
        }
        assert_round(&url, &owner_pool, &roles, label, round).await;
        for role in &roles {
            cleanup_role(&admin, role).await;
        }
    }

    owner_pool.close().await;
    cleanup_role(&admin, &owner).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn parallel_grants_for_one_role_and_for_two_distinct_roles_all_succeed() {
    run_rounds("one role", 1).await;
    run_rounds("two roles", 2).await;
}
