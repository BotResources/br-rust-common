use sqlx::PgPool;

use crate::catalog_race::retry_catalog_race;
use crate::names::{assert_plain_identifier, generated_password, unique_role_name};
use crate::pools::open_pool_as;
use crate::verifier::scram_verifier;

const PUBLIC_SCHEMA_ACL_LOCK: i64 = 0x6272_5f74_6573_7431;

pub struct Caller {
    pub pool: PgPool,
    pub role: String,
    pub password: String,
}

pub async fn cleanup_role(admin: &PgPool, role: &str) {
    assert_plain_identifier(role);
    let drop_owned = format!("DROP OWNED BY \"{role}\" CASCADE");
    let _ =
        retry_catalog_race(|| async { sqlx::query(&drop_owned).execute(admin).await.map(|_| ()) })
            .await;
    let _ = sqlx::query(&format!("DROP ROLE IF EXISTS \"{role}\""))
        .execute(admin)
        .await;
}

pub async fn setup_caller(admin: &PgPool, admin_url: &str) -> Caller {
    create_caller(admin, admin_url, unique_role_name()).await
}

pub(crate) async fn create_caller(admin: &PgPool, admin_url: &str, role: String) -> Caller {
    assert_plain_identifier(&role);
    let password = generated_password();
    let verifier = scram_verifier(&password);
    sqlx::query(&format!(
        "CREATE ROLE \"{role}\" LOGIN CREATEROLE NOSUPERUSER PASSWORD '{verifier}'"
    ))
    .execute(admin)
    .await
    .expect("create caller role");

    let grant_create = format!("GRANT CREATE ON SCHEMA public TO \"{role}\"");
    retry_catalog_race(|| async {
        let mut tx = admin.begin().await?;
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(PUBLIC_SCHEMA_ACL_LOCK)
            .execute(&mut *tx)
            .await?;
        sqlx::query(&grant_create).execute(&mut *tx).await?;
        tx.commit().await
    })
    .await
    .expect("grant CREATE on public to caller");

    let pool = open_pool_as(admin_url, &role, &password)
        .await
        .expect("connect as caller");
    Caller {
        pool,
        role,
        password,
    }
}
