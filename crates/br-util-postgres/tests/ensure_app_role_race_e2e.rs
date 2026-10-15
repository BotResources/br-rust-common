use std::sync::Arc;

use br_test_support::{TestWorld, generated_password, wide_caller_pool, with_test_world};
use br_util_postgres::ensure_app_role;
use sqlx::PgPool;
use tokio::sync::Barrier;

const PARALLEL_CALLS: usize = 16;
const ROUNDS: usize = 5;
const DATABASES: usize = 3;
const CROSS_DATABASE_ROUNDS: usize = 20;

struct Call {
    pool: PgPool,
    role: String,
    password: String,
}

async fn race(calls: Vec<Call>) -> Vec<Result<(), String>> {
    let barrier = Arc::new(Barrier::new(calls.len()));
    let handles: Vec<_> = calls
        .into_iter()
        .map(|call| {
            let barrier = Arc::clone(&barrier);
            tokio::spawn(async move {
                barrier.wait().await;
                ensure_app_role(&call.pool, &call.role, &call.password)
                    .await
                    .map_err(|e| e.to_string())
            })
        })
        .collect();
    let mut results = Vec::with_capacity(handles.len());
    for handle in handles {
        results.push(handle.await.expect("task must not panic"));
    }
    results
}

fn failures(results: &[Result<(), String>]) -> Vec<&String> {
    results.iter().filter_map(|r| r.as_ref().err()).collect()
}

async fn assert_role_exists(world: &TestWorld, role: &str, context: &str) {
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = $1)")
            .bind(role)
            .fetch_one(world.admin())
            .await
            .expect("role lookup");
    assert!(exists, "{context}: role must exist after the calls");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser with password authentication (not trust)"]
async fn parallel_first_calls_for_the_same_role_all_succeed() {
    with_test_world(|world| async move {
        let caller = world.caller().await;
        let pool = wide_caller_pool(
            world.url(),
            &caller.role,
            &caller.password,
            PARALLEL_CALLS as u32,
        )
        .await;

        for round in 0..ROUNDS {
            let role = world.unique_role();
            let password = generated_password();
            let calls = (0..PARALLEL_CALLS)
                .map(|_| Call {
                    pool: pool.clone(),
                    role: role.clone(),
                    password: password.clone(),
                })
                .collect();
            let results = race(calls).await;
            let failed = failures(&results);
            assert!(
                failed.is_empty(),
                "round {round}: {} of {PARALLEL_CALLS} parallel first calls failed: {failed:?}",
                failed.len()
            );
            assert_role_exists(&world, &role, &format!("round {round}")).await;
        }

        pool.close().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser with password authentication (not trust)"]
async fn parallel_first_calls_for_two_different_roles_in_one_database_all_succeed() {
    with_test_world(|world| async move {
        let caller = world.caller().await;
        let pool = wide_caller_pool(world.url(), &caller.role, &caller.password, 2).await;

        for round in 0..ROUNDS {
            let roles = [world.unique_role(), world.unique_role()];
            let calls = roles
                .iter()
                .map(|role| Call {
                    pool: pool.clone(),
                    role: role.clone(),
                    password: generated_password(),
                })
                .collect();
            let results = race(calls).await;
            let failed = failures(&results);
            assert!(
                failed.is_empty(),
                "round {round}: {} of 2 parallel calls for different roles failed: {failed:?}",
                failed.len()
            );
            for role in &roles {
                assert_role_exists(&world, role, &format!("round {round}")).await;
            }
        }

        pool.close().await;
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser with password authentication (not trust)"]
async fn one_first_call_per_database_for_the_same_role_all_succeed() {
    with_test_world(|world| async move {
        let caller = world.caller().await;
        let mut pools = Vec::new();
        for _ in 0..DATABASES {
            let database = world.isolated_database().await;
            pools.push(wide_caller_pool(database.url(), &caller.role, &caller.password, 1).await);
        }

        for round in 0..CROSS_DATABASE_ROUNDS {
            let role = world.unique_role();
            let password = generated_password();
            let calls = pools
                .iter()
                .map(|pool| Call {
                    pool: pool.clone(),
                    role: role.clone(),
                    password: password.clone(),
                })
                .collect();
            let results = race(calls).await;
            let failed = failures(&results);
            assert!(
                failed.is_empty(),
                "round {round}: {} of {DATABASES} cross-database first calls failed: {failed:?}",
                failed.len()
            );
            assert_role_exists(&world, &role, &format!("round {round}")).await;
        }

        for pool in &pools {
            pool.close().await;
        }
    })
    .await;
}
