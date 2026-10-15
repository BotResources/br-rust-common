use std::str::FromStr;
use std::sync::Arc;

use br_test_support::{cleanup_role, require_test_db_url, setup_caller, unique_role_name};
use br_util_postgres::ensure_app_role;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use tokio::sync::Barrier;
use uuid::Uuid;

const CALLER_PW: &str = "caller_pw_for_e2e_only";
const PARALLEL_CALLS: usize = 16;
const ROUNDS: usize = 5;

fn generated_password() -> String {
    format!("pw_{}", Uuid::now_v7().simple())
}

async fn wide_caller_pool(url: &str, caller: &str, max_connections: u32) -> PgPool {
    let options = PgConnectOptions::from_str(url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(caller)
        .password(CALLER_PW);
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
        .expect("connect as caller")
}

async fn caller_pool_on_database(
    url: &str,
    caller: &str,
    database: &str,
    max_connections: u32,
) -> PgPool {
    let options = PgConnectOptions::from_str(url)
        .expect("TEST_DATABASE_URL must parse as a Postgres URL")
        .username(caller)
        .password(CALLER_PW)
        .database(database);
    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect_with(options)
        .await
        .expect("connect as caller")
}

async fn parallel_first_calls(
    pools: &[PgPool],
    role: &str,
    password: &str,
    calls: usize,
) -> Vec<Result<(), String>> {
    let barrier = Arc::new(Barrier::new(calls));
    let handles: Vec<_> = (0..calls)
        .map(|i| {
            let pool = pools[i % pools.len()].clone();
            let barrier = Arc::clone(&barrier);
            let role = role.to_string();
            let password = password.to_string();
            tokio::spawn(async move {
                barrier.wait().await;
                ensure_app_role(&pool, &role, &password)
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

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn parallel_first_calls_for_the_same_role_all_succeed() {
    let url = require_test_db_url();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect as admin");
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    caller_pool.close().await;
    let pool = wide_caller_pool(&url, &caller, PARALLEL_CALLS as u32).await;

    for round in 0..ROUNDS {
        let role = unique_role_name();
        let password = generated_password();
        let results = parallel_first_calls(
            std::slice::from_ref(&pool),
            &role,
            &password,
            PARALLEL_CALLS,
        )
        .await;
        let failures: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();
        assert!(
            failures.is_empty(),
            "round {round}: {} of {PARALLEL_CALLS} parallel first calls failed: {failures:?}",
            failures.len()
        );
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
                .bind(&role)
                .fetch_one(&admin)
                .await
                .expect("role lookup");
        assert!(exists, "round {round}: role must exist after the calls");
        cleanup_role(&admin, &role).await;
    }

    pool.close().await;
    cleanup_role(&admin, &caller).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn parallel_first_calls_from_two_databases_of_one_cluster_all_succeed() {
    let url = require_test_db_url();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .expect("connect as admin");
    let (caller_pool, caller) = setup_caller(&admin, &url).await;
    caller_pool.close().await;

    let databases = [unique_role_name(), unique_role_name()];
    for database in &databases {
        sqlx::query(&format!("CREATE DATABASE \"{database}\""))
            .execute(&admin)
            .await
            .expect("create database");
    }
    let mut pools = Vec::new();
    for database in &databases {
        pools.push(caller_pool_on_database(&url, &caller, database, PARALLEL_CALLS as u32).await);
    }

    for round in 0..ROUNDS {
        let role = unique_role_name();
        let password = generated_password();
        let results = parallel_first_calls(&pools, &role, &password, PARALLEL_CALLS).await;
        let failures: Vec<_> = results.iter().filter_map(|r| r.as_ref().err()).collect();
        assert!(
            failures.is_empty(),
            "round {round}: {} of {PARALLEL_CALLS} cross-database first calls failed: {failures:?}",
            failures.len()
        );
        cleanup_role(&admin, &role).await;
    }

    for pool in &pools {
        pool.close().await;
    }
    for database in &databases {
        sqlx::query(&format!(
            "DROP DATABASE IF EXISTS \"{database}\" WITH (FORCE)"
        ))
        .execute(&admin)
        .await
        .expect("drop database");
    }
    cleanup_role(&admin, &caller).await;
}
