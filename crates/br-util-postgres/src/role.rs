use sqlx::PgPool;

use crate::error::PostgresError;
use crate::role_password::{alter_role_verifier_sql, scram_verifier};
use crate::role_retry::retry_concurrent_role_ddl;

const MAX_ROLE_NAME_LEN: usize = 63;
const ADVISORY_LOCK_NAMESPACE: &str = "br_util_postgres.ensure_app_role:";

pub(crate) fn validate_role_name(name: &str) -> Result<(), PostgresError> {
    if is_valid_role_name(name) {
        Ok(())
    } else {
        Err(PostgresError::InvalidRoleName(name.to_string()))
    }
}

fn is_valid_role_name(name: &str) -> bool {
    if name.is_empty() || name.len() > MAX_ROLE_NAME_LEN {
        return false;
    }
    let mut chars = name.chars();
    let first = chars.next().expect("non-empty checked above");
    if !first.is_ascii_lowercase() {
        return false;
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub async fn ensure_app_role(
    pool: &PgPool,
    role_name: &str,
    password: &str,
) -> Result<(), PostgresError> {
    validate_role_name(role_name)?;
    let alter_sql = alter_role_verifier_sql(role_name, &scram_verifier(password)?)?;
    retry_concurrent_role_ddl(|| ensure_app_role_once(pool, role_name, &alter_sql)).await
}

async fn ensure_app_role_once(
    pool: &PgPool,
    role_name: &str,
    alter_sql: &str,
) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;

    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(format!("{ADVISORY_LOCK_NAMESPACE}{role_name}"))
        .execute(&mut *tx)
        .await?;

    let create_sql = format!(
        "DO $$ BEGIN \
           IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{role_name}') THEN \
             CREATE ROLE \"{role_name}\" LOGIN; \
           END IF; \
         END $$"
    );
    sqlx::query(&create_sql).execute(&mut *tx).await?;

    sqlx::query(alter_sql)
        .persistent(false)
        .execute(&mut *tx)
        .await?;

    tx.commit().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_lowercase_name() {
        assert!(is_valid_role_name("app"));
        assert!(is_valid_role_name("myservice_app"));
    }

    #[test]
    fn accepts_digits_and_underscore_after_first_char() {
        assert!(is_valid_role_name("a"));
        assert!(is_valid_role_name("a1"));
        assert!(is_valid_role_name("a_1"));
        assert!(is_valid_role_name("acme_app_v2"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_valid_role_name(""));
    }

    #[test]
    fn rejects_leading_digit_or_underscore() {
        assert!(!is_valid_role_name("1app"));
        assert!(!is_valid_role_name("_app"));
    }

    #[test]
    fn rejects_uppercase() {
        assert!(!is_valid_role_name("App"));
        assert!(!is_valid_role_name("appA"));
    }

    #[test]
    fn rejects_special_chars() {
        assert!(!is_valid_role_name("app-name"));
        assert!(!is_valid_role_name("app.name"));
        assert!(!is_valid_role_name("app name"));
        assert!(!is_valid_role_name("app\"; DROP TABLE users;--"));
        assert!(!is_valid_role_name("app'; --"));
    }

    #[test]
    fn rejects_over_63_bytes() {
        let ok = "a".repeat(63);
        let too_long = "a".repeat(64);
        assert!(is_valid_role_name(&ok));
        assert!(!is_valid_role_name(&too_long));
    }
}
