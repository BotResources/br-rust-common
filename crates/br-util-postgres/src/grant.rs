use sqlx::PgPool;

use crate::error::PostgresError;
use crate::role::validate_role_name;

pub async fn grant_app_access(pool: &PgPool, app_role: &str) -> Result<(), PostgresError> {
    validate_role_name(app_role)?;

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
        sqlx::query(&sql)
            .execute(pool)
            .await
            .map_err(PostgresError::Db)?;
    }
    Ok(())
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::role::ensure_app_role;
    use crate::test_support::{
        cleanup_role, open_pool_as, setup_caller, test_db_url, unique_role_name, unique_table_name,
    };
    use sqlx::Row;
    use sqlx::postgres::PgPoolOptions;

    const APP_PW: &str = "app_pw_for_e2e_only";

    async fn bootstrap(url: &str) -> (sqlx::PgPool, sqlx::PgPool, String, String) {
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(url)
            .await
            .expect("connect as admin");
        let (owner_pool, owner) = setup_caller(&admin, url).await;
        let app_role = unique_role_name();
        ensure_app_role(&owner_pool, &app_role, APP_PW)
            .await
            .expect("ensure app role");
        (admin, owner_pool, owner, app_role)
    }

    async fn teardown(
        admin: sqlx::PgPool,
        owner_pool: sqlx::PgPool,
        owner: String,
        app_role: String,
    ) {
        owner_pool.close().await;
        cleanup_role(&admin, &app_role).await;
        cleanup_role(&admin, &owner).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_grants_table_dml() {
        let Some(url) = test_db_url() else { return };
        let (admin, owner_pool, owner, app_role) = bootstrap(&url).await;
        let table = unique_table_name();

        sqlx::query(&format!("CREATE TABLE \"{table}\" (id int, val text)"))
            .execute(&owner_pool)
            .await
            .expect("create table");
        sqlx::query(&format!(
            "INSERT INTO \"{table}\" (id, val) VALUES (1, 'a'), (2, 'b')"
        ))
        .execute(&owner_pool)
        .await
        .expect("insert seed");

        let app_pool_before = open_pool_as(&url, &app_role, APP_PW)
            .await
            .expect("app login before grant");
        let before = sqlx::query(&format!("SELECT id FROM \"{table}\""))
            .fetch_all(&app_pool_before)
            .await;
        assert!(
            before.is_err(),
            "SELECT must be permission-denied before grant_app_access"
        );
        app_pool_before.close().await;

        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("grant_app_access");

        let app_pool = open_pool_as(&url, &app_role, APP_PW)
            .await
            .expect("app login after grant");
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
        teardown(admin, owner_pool, owner, app_role).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_covers_tables_created_after_grant() {
        let Some(url) = test_db_url() else { return };
        let (admin, owner_pool, owner, app_role) = bootstrap(&url).await;

        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("grant_app_access");

        let late_table = unique_table_name();
        sqlx::query(&format!("CREATE TABLE \"{late_table}\" (id int)"))
            .execute(&owner_pool)
            .await
            .expect("create late table");
        sqlx::query(&format!("INSERT INTO \"{late_table}\" (id) VALUES (42)"))
            .execute(&owner_pool)
            .await
            .expect("insert into late table");

        let app_pool = open_pool_as(&url, &app_role, APP_PW)
            .await
            .expect("app login");
        let row = sqlx::query(&format!("SELECT id FROM \"{late_table}\""))
            .fetch_one(&app_pool)
            .await
            .expect("SELECT on table created after grant_app_access");
        let id: i32 = row.try_get("id").expect("id column");
        assert_eq!(id, 42);

        app_pool.close().await;
        teardown(admin, owner_pool, owner, app_role).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_grants_sequence_usage() {
        let Some(url) = test_db_url() else { return };
        let (admin, owner_pool, owner, app_role) = bootstrap(&url).await;
        let table = unique_table_name();

        sqlx::query(&format!(
            "CREATE TABLE \"{table}\" (id SERIAL PRIMARY KEY, val text)"
        ))
        .execute(&owner_pool)
        .await
        .expect("create serial table");

        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("grant_app_access");

        let app_pool = open_pool_as(&url, &app_role, APP_PW)
            .await
            .expect("app login");
        let row = sqlx::query(&format!(
            "INSERT INTO \"{table}\" (val) VALUES ('a') RETURNING id"
        ))
        .fetch_one(&app_pool)
        .await
        .expect("INSERT into SERIAL table");
        let id: i32 = row.try_get("id").expect("id column");
        assert!(id >= 1);

        app_pool.close().await;
        teardown(admin, owner_pool, owner, app_role).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_is_idempotent() {
        let Some(url) = test_db_url() else { return };
        let (admin, owner_pool, owner, app_role) = bootstrap(&url).await;

        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("first grant");
        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("second grant must be a no-op");

        teardown(admin, owner_pool, owner, app_role).await;
    }
}
