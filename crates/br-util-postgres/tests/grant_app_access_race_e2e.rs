use std::sync::Arc;

use br_test_support::{
    IsolatedDatabase, TestWorld, generated_password, open_pool_as, unique_table_name,
    wide_caller_pool, with_test_world,
};
use br_util_postgres::{ensure_app_role, grant_app_access};
use sqlx::PgPool;
use tokio::sync::Barrier;

const PARALLEL_CALLS: usize = 16;
const ROUNDS: usize = 5;

async fn parallel_grants(
    pool: &PgPool,
    roles: &[(String, String)],
    calls: usize,
) -> Vec<Result<(), String>> {
    let barrier = Arc::new(Barrier::new(calls));
    let handles: Vec<_> = (0..calls)
        .map(|i| {
            let pool = pool.clone();
            let barrier = Arc::clone(&barrier);
            let role = roles[i % roles.len()].0.clone();
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

async fn assert_round(
    database: &IsolatedDatabase,
    owner_pool: &PgPool,
    roles: &[(String, String)],
    label: &str,
    round: usize,
) {
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
    for (role, password) in roles {
        let app_pool = open_pool_as(database.url(), role, password)
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

async fn run_rounds(world: &TestWorld, label: &str, role_count: usize) {
    let database = world.isolated_database().await;
    let owner = world.caller_in(&database).await;
    let owner_pool = wide_caller_pool(
        database.url(),
        &owner.role,
        &owner.password,
        PARALLEL_CALLS as u32,
    )
    .await;
    owner.pool.close().await;

    for round in 0..ROUNDS {
        let mut roles = Vec::new();
        for _ in 0..role_count {
            let role = world.unique_role();
            let password = generated_password();
            ensure_app_role(&owner_pool, &role, &password)
                .await
                .expect("ensure app role");
            roles.push((role, password));
        }
        assert_round(&database, &owner_pool, &roles, label, round).await;
    }

    owner_pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
async fn parallel_grants_for_one_role_and_for_two_distinct_roles_all_succeed() {
    with_test_world(|world| async move {
        run_rounds(&world, "one role", 1).await;
        run_rounds(&world, "two roles", 2).await;
    })
    .await;
}
