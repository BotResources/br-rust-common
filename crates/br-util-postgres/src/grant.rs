use sqlx::PgPool;

use crate::error::PostgresError;
use crate::role::validate_role_name;

/// Grant table access to the application database role after migrations.
///
/// Tables created by migrations are owned by the migration role and not
/// accessible to the app role until explicitly granted. This function does
/// two things, both idempotent:
///
/// 1. **Bulk grant on existing objects** — `GRANT USAGE/SELECT/INSERT/
///    UPDATE/DELETE` on all tables and `USAGE, SELECT` on all sequences
///    currently in `public`.
/// 2. **Default privileges for future objects** — `ALTER DEFAULT PRIVILEGES
///    IN SCHEMA public GRANT … TO <app_role>` so that any *future* table or
///    sequence created by the current role (typically the owner running
///    later migrations) is automatically GRANTed to the app role. Without
///    this, a subsequent migration creating a new table would leave the app
///    role unable to touch it until a redeploy re-ran step 1.
///
/// **Must be invoked via the same pool that runs migrations.** Default
/// privileges attach to the role executing the statement (`CURRENT_USER`),
/// so calling this through the app pool would set defaults for objects the
/// app role creates — which it can't, making the call a silent no-op.
///
/// The `app_role` parameter is project-specific (e.g., "hanshow_app",
/// "medisup_app"). It is validated against `^[a-z][a-z0-9_]*$` (≤63 bytes)
/// before being interpolated into DDL; invalid names return
/// [`PostgresError::InvalidRoleName`] without touching the database.
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

/// Live-Postgres tests for `grant_app_access`.
///
/// Same gating + setup as the live tests in role.rs: ignored by default,
/// run in the `e2e-postgres` CI job (or locally with `TEST_DATABASE_URL` +
/// `cargo test -- --ignored`) against a fresh `caller_<uuid>` role
/// configured exactly like CNPG's `<svc>_owner` (`LOGIN CREATEROLE
/// NOSUPERUSER`). The owner is what would, in production, create the
/// app role via `ensure_app_role`, run migrations, and call
/// `grant_app_access` — so the tests run through that owner pool.
///
/// Covers the same risk class as issue #13: every line of DDL here can
/// fail or silently misgrant in ways that no unit test would catch, and
/// the production blast radius is "app role can't touch a new table
/// shipped by a migration".
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

    /// Bootstrap: admin → owner → app role created by owner. Returns the
    /// admin pool (for cleanup), the owner pool (for migrations + grants),
    /// and the names of the two roles so the test can drop them.
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
        // App role first (no objects), then owner (owns the tables/seqs
        // the test created — DROP OWNED inside cleanup_role takes them).
        cleanup_role(&admin, &app_role).await;
        cleanup_role(&admin, &owner).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn grant_app_access_grants_table_dml() {
        let Some(url) = test_db_url() else { return };
        let (admin, owner_pool, owner, app_role) = bootstrap(&url).await;
        let table = unique_table_name();

        // Owner creates a table populated with two rows.
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

        // Before grant: app role can log in but a SELECT on the table is
        // permission denied. Proves the grant is load-bearing — without
        // it, the post-grant SELECT below would pass for the wrong reason
        // (e.g., PG defaulting public schema to PUBLIC role).
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

        // After grant: SELECT/INSERT/UPDATE/DELETE all succeed.
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
        // Defense in depth: the ALTER DEFAULT PRIVILEGES half of
        // grant_app_access. Production calls grant_app_access ONCE on
        // first boot; later migrations create new tables that must be
        // grantable to the app role without re-running grant_app_access.
        // If ALTER DEFAULT PRIVILEGES regresses (wrong role assertion,
        // wrong schema, missing TABLE clause, etc.), this fires.
        let Some(url) = test_db_url() else { return };
        let (admin, owner_pool, owner, app_role) = bootstrap(&url).await;

        grant_app_access(&owner_pool, &app_role)
            .await
            .expect("grant_app_access");

        // Now create a table AFTER the grant. Default privileges should
        // auto-attach.
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
        // Sequences need USAGE+SELECT for nextval()/currval() to work
        // from the app role. SERIAL/IDENTITY columns produce sequences;
        // without this grant, app-side INSERTs that rely on serial keys
        // would fail with "permission denied for sequence ...".
        let Some(url) = test_db_url() else { return };
        let (admin, owner_pool, owner, app_role) = bootstrap(&url).await;
        let table = unique_table_name();

        // SERIAL implicitly creates a sequence owned by the column.
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
        // INSERT that defaults the SERIAL — this is what exercises the
        // sequence USAGE grant.
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
        // Production calls grant_app_access on every boot, after the
        // migration step. Re-running it must be a no-op, not an error.
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
