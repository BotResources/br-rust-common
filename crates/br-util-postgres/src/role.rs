use sqlx::PgPool;

use crate::error::PostgresError;

/// Postgres identifier length cap. NAMEDATALEN is 64; the usable length is 63.
const MAX_ROLE_NAME_LEN: usize = 63;

/// Validate a role name against `^[a-z][a-z0-9_]*$` with a 63-byte cap.
///
/// Both `ensure_app_role` and `grant_app_access` interpolate the role name
/// into DDL via `format!` (Postgres does not accept bind parameters for
/// identifiers), so this check is the sole barrier against SQL injection
/// through the role identifier. Keep the allowed alphabet narrow — there is
/// no use case for quoted weird-character roles in this codebase.
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

/// Idempotently create the application role and set its password.
///
/// Intended for service startup, before `sqlx::migrate!()` runs, using a
/// pool connected as the **owner** role. The two-role model is:
/// - owner — runs DDL, BYPASSRLS, creates the app role
/// - app — runtime queries, RLS-enforced
///
/// Behavior:
/// 1. Validates `role_name` against `^[a-z][a-z0-9_]*$` (≤63 bytes). The name
///    is interpolated into DDL — invalid names return
///    [`PostgresError::InvalidRoleName`] without touching the database.
/// 2. Executes a `DO $$ ... END $$` block that runs `CREATE ROLE ... LOGIN`
///    only when the role does not already exist. Safe to call on every
///    startup. `CREATE ROLE` defaults to no-privilege (NOSUPERUSER,
///    NOCREATEDB, NOCREATEROLE, NOBYPASSRLS, NOREPLICATION, INHERIT), so no
///    follow-up attribute hardening is needed — and attempting one would
///    require SUPERUSER (PG 16+ rejects NOSUPERUSER/NOBYPASSRLS/NOREPLICATION
///    from non-superuser CREATEROLE callers even as a no-op).
/// 3. Runs `ALTER ROLE "<name>" PASSWORD $1` with `password` bound as a
///    parameter — the secret never enters the SQL text. Setting the password
///    only needs membership in the target role, which the CREATEROLE creator
///    receives implicitly.
///
/// The DO block runs as a single statement so the existence check and the
/// `CREATE ROLE` happen in the same snapshot, which avoids the race where
/// two concurrent startups both observe "not exists" and the second one
/// errors with `role already exists`.
///
/// ```ignore
/// let owner_pool = init_migration_pool(env, allow_insecure).await?;
/// ensure_app_role(&owner_pool, "myservice_app", &app_password).await?;
/// sqlx::migrate!().run(&owner_pool).await?;
/// drop(owner_pool);
/// let app_pool = init_pool(&app_url, env, allow_insecure).await?;
/// ```
pub async fn ensure_app_role(
    pool: &PgPool,
    role_name: &str,
    password: &str,
) -> Result<(), PostgresError> {
    validate_role_name(role_name)?;

    // role_name validated above — restricted to [a-z][a-z0-9_]* so neither
    // the single-quoted literal nor the double-quoted identifier can be
    // escaped. The DO block is the standard CREATE-IF-NOT-EXISTS idiom for
    // roles, which Postgres does not support as native syntax.
    let create_sql = format!(
        "DO $$ BEGIN \
           IF NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = '{role_name}') THEN \
             CREATE ROLE \"{role_name}\" LOGIN; \
           END IF; \
         END $$"
    );
    sqlx::query(&create_sql)
        .execute(pool)
        .await
        .map_err(PostgresError::Db)?;

    let alter_sql = format!("ALTER ROLE \"{role_name}\" PASSWORD $1");
    sqlx::query(&alter_sql)
        .bind(password)
        .execute(pool)
        .await
        .map_err(PostgresError::Db)?;

    Ok(())
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
        assert!(is_valid_role_name("hanshow_app_v2"));
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
