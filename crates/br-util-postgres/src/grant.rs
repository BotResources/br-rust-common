use sqlx::PgPool;

use crate::advisory_lock::lock_transaction_scope;
use crate::catalog_retry::retry_concurrent_catalog_ddl;
use crate::error::PostgresError;
use crate::role::validate_role_name;

const ADVISORY_LOCK_KEY: &str = "br_util_postgres.grant_app_access:public";

pub async fn grant_app_access(pool: &PgPool, app_role: &str) -> Result<(), PostgresError> {
    validate_role_name(app_role)?;
    retry_concurrent_catalog_ddl(|| grant_app_access_once(pool, app_role)).await
}

async fn grant_app_access_once(pool: &PgPool, app_role: &str) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    lock_transaction_scope(&mut tx, ADVISORY_LOCK_KEY).await?;

    for sql in [
        format!("GRANT USAGE ON SCHEMA public TO \"{app_role}\""),
        format!(
            "GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO \"{app_role}\""
        ),
        format!("GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO \"{app_role}\""),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT SELECT, INSERT, UPDATE, DELETE ON TABLES TO \"{app_role}\""
        ),
        format!(
            "ALTER DEFAULT PRIVILEGES IN SCHEMA public \
             GRANT USAGE, SELECT ON SEQUENCES TO \"{app_role}\""
        ),
    ] {
        sqlx::query(&sql).execute(&mut *tx).await?;
    }

    tx.commit().await
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::role::ensure_app_role;
    use br_test_support::{
        Caller, IsolatedDatabase, TestWorld, generated_password, open_pool_as, unique_table_name,
        with_test_world,
    };
    use sqlx::Row;

    struct Fixture {
        database: IsolatedDatabase,
        owner: Caller,
        app_role: String,
        app_password: String,
    }

    impl Fixture {
        async fn new(world: &TestWorld) -> Self {
            let database = world.isolated_database().await;
            let owner = world.caller_in(&database).await;
            let app_role = world.unique_role();
            let app_password = generated_password();
            ensure_app_role(&owner.pool, &app_role, &app_password)
                .await
                .expect("ensure app role");
            Self {
                database,
                owner,
                app_role,
                app_password,
            }
        }

        async fn app_pool(&self) -> sqlx::PgPool {
            open_pool_as(self.database.url(), &self.app_role, &self.app_password)
                .await
                .expect("app login")
        }
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_grants_table_dml() {
        with_test_world(|world| async move {
            let fixture = Fixture::new(&world).await;
            let owner_pool = &fixture.owner.pool;
            let app_role = &fixture.app_role;
            let table = unique_table_name();

            sqlx::query(&format!("CREATE TABLE \"{table}\" (id int, val text)"))
                .execute(owner_pool)
                .await
                .expect("create table");
            sqlx::query(&format!(
                "INSERT INTO \"{table}\" (id, val) VALUES (1, 'a'), (2, 'b')"
            ))
            .execute(owner_pool)
            .await
            .expect("insert seed");

            let app_pool_before = fixture.app_pool().await;
            let before = sqlx::query(&format!("SELECT id FROM \"{table}\""))
                .fetch_all(&app_pool_before)
                .await;
            assert!(
                before.is_err(),
                "SELECT must be permission-denied before grant_app_access"
            );
            app_pool_before.close().await;

            grant_app_access(owner_pool, app_role)
                .await
                .expect("grant_app_access");

            let app_pool = fixture.app_pool().await;
            let rows = sqlx::query(&format!("SELECT id FROM \"{table}\" ORDER BY id"))
                .fetch_all(&app_pool)
                .await
                .expect("SELECT after grant");
            assert_eq!(rows.len(), 2);
            sqlx::query(&format!(
                "INSERT INTO \"{table}\" (id, val) VALUES (3, 'c')"
            ))
            .execute(&app_pool)
            .await
            .expect("INSERT after grant");
            sqlx::query(&format!("UPDATE \"{table}\" SET val = 'x' WHERE id = 1"))
                .execute(&app_pool)
                .await
                .expect("UPDATE after grant");
            sqlx::query(&format!("DELETE FROM \"{table}\" WHERE id = 2"))
                .execute(&app_pool)
                .await
                .expect("DELETE after grant");

            app_pool.close().await;
            fixture.owner.pool.close().await;
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_covers_tables_created_after_grant() {
        with_test_world(|world| async move {
            let fixture = Fixture::new(&world).await;
            let owner_pool = &fixture.owner.pool;
            let app_role = &fixture.app_role;

            grant_app_access(owner_pool, app_role)
                .await
                .expect("grant_app_access");

            let late_table = unique_table_name();
            sqlx::query(&format!("CREATE TABLE \"{late_table}\" (id int)"))
                .execute(owner_pool)
                .await
                .expect("create late table");
            sqlx::query(&format!("INSERT INTO \"{late_table}\" (id) VALUES (42)"))
                .execute(owner_pool)
                .await
                .expect("insert into late table");

            let app_pool = fixture.app_pool().await;
            let row = sqlx::query(&format!("SELECT id FROM \"{late_table}\""))
                .fetch_one(&app_pool)
                .await
                .expect("SELECT on table created after grant_app_access");
            let id: i32 = row.try_get("id").expect("id column");
            assert_eq!(id, 42);

            app_pool.close().await;
            fixture.owner.pool.close().await;
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_grants_sequence_usage() {
        with_test_world(|world| async move {
            let fixture = Fixture::new(&world).await;
            let owner_pool = &fixture.owner.pool;
            let app_role = &fixture.app_role;
            let table = unique_table_name();

            sqlx::query(&format!(
                "CREATE TABLE \"{table}\" (id SERIAL PRIMARY KEY, val text)"
            ))
            .execute(owner_pool)
            .await
            .expect("create serial table");

            grant_app_access(owner_pool, app_role)
                .await
                .expect("grant_app_access");

            let app_pool = fixture.app_pool().await;
            let row = sqlx::query(&format!(
                "INSERT INTO \"{table}\" (val) VALUES ('a') RETURNING id"
            ))
            .fetch_one(&app_pool)
            .await
            .expect("INSERT into SERIAL table");
            let id: i32 = row.try_get("id").expect("id column");
            assert!(id >= 1);

            app_pool.close().await;
            fixture.owner.pool.close().await;
        })
        .await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_is_idempotent() {
        with_test_world(|world| async move {
            let fixture = Fixture::new(&world).await;
            let owner_pool = &fixture.owner.pool;
            let app_role = &fixture.app_role;

            grant_app_access(owner_pool, app_role)
                .await
                .expect("first grant");
            grant_app_access(owner_pool, app_role)
                .await
                .expect("second grant must be a no-op");

            fixture.owner.pool.close().await;
        })
        .await;
    }
}
