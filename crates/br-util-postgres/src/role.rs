use sqlx::PgPool;
use uuid::Uuid;

use crate::error::PostgresError;

const MAX_ROLE_NAME_LEN: usize = 63;

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

    let tag = fresh_dollar_quote_tag();
    let alter_sql = build_alter_password_sql(role_name, password, &tag);
    let result = sqlx::query(&alter_sql).execute(pool).await;
    drop(scrub(alter_sql));
    result.map_err(PostgresError::Db)?;

    Ok(())
}

pub(crate) fn build_alter_password_sql(role_name: &str, password: &str, tag: &str) -> String {
    assert!(
        is_valid_role_name(role_name),
        "role_name must be pre-validated by validate_role_name"
    );
    assert!(
        is_valid_dollar_quote_tag(tag),
        "tag must match Postgres identifier rules"
    );
    let delimiter = format!("${tag}$");
    assert!(
        !password.contains(&delimiter),
        "fresh UUID-v7 tag collided with password content — cryptographically impossible; \
         check your RNG"
    );
    format!("ALTER ROLE \"{role_name}\" PASSWORD {delimiter}{password}{delimiter}")
}

fn fresh_dollar_quote_tag() -> String {
    format!("br_{}", Uuid::now_v7().simple())
}

fn is_valid_dollar_quote_tag(tag: &str) -> bool {
    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn scrub(mut s: String) -> String {
    // SAFETY: we replace every byte with 0, which is valid UTF-8 (the NUL
    // codepoint is a single 0 byte) — the resulting string is still
    // well-formed UTF-8.
    unsafe {
        for b in s.as_bytes_mut() {
            *b = 0;
        }
    }
    s
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

    const TEST_TAG: &str = "br_test";

    #[test]
    fn builds_dollar_quoted_password_literal() {
        let sql = build_alter_password_sql("app", "s3cret", TEST_TAG);
        assert_eq!(sql, "ALTER ROLE \"app\" PASSWORD $br_test$s3cret$br_test$");
    }

    #[test]
    fn does_not_emit_bind_parameters() {
        let sql = build_alter_password_sql("app", "hunter2", TEST_TAG);
        assert!(
            !sql.contains("$1"),
            "SQL must not contain a bind placeholder: {sql}"
        );
    }

    #[test]
    fn passes_special_characters_through_verbatim() {
        let pw = r#"'"\;-- $ $$ $foo$ ${tag}"#;
        let sql = build_alter_password_sql("app", pw, TEST_TAG);
        assert_eq!(
            sql,
            format!("ALTER ROLE \"app\" PASSWORD $br_test${pw}$br_test$")
        );
    }

    #[test]
    #[should_panic(expected = "cryptographically impossible")]
    fn panics_when_password_contains_closing_delimiter() {
        build_alter_password_sql("app", "anything$br_test$DROP TABLE", TEST_TAG);
    }

    #[test]
    #[should_panic(expected = "role_name must be pre-validated")]
    fn panics_on_unvalidated_role_name() {
        build_alter_password_sql("bad-name", "pw", TEST_TAG);
    }

    #[test]
    fn fresh_tag_is_a_valid_postgres_identifier() {
        let tag = fresh_dollar_quote_tag();
        assert!(tag.starts_with("br_"));
        assert!(is_valid_dollar_quote_tag(&tag), "tag={tag}");
    }

    #[test]
    fn fresh_tag_differs_between_calls() {
        let a = fresh_dollar_quote_tag();
        let b = fresh_dollar_quote_tag();
        assert_ne!(a, b);
    }

    #[test]
    fn accepts_well_formed_tags() {
        assert!(is_valid_dollar_quote_tag("br_"));
        assert!(is_valid_dollar_quote_tag("br_abc123"));
        assert!(is_valid_dollar_quote_tag("_x"));
        assert!(is_valid_dollar_quote_tag("Tag1"));
    }

    #[test]
    fn rejects_malformed_tags() {
        assert!(!is_valid_dollar_quote_tag(""));
        assert!(!is_valid_dollar_quote_tag("1abc"));
        assert!(!is_valid_dollar_quote_tag("a b"));
        assert!(!is_valid_dollar_quote_tag("a-b"));
        assert!(!is_valid_dollar_quote_tag("a$b"));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    use br_test_support::{cleanup_role, setup_caller, test_db_url, unique_role_name};
    use sqlx::Connection;
    use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
    use std::str::FromStr;

    async fn try_login(
        admin_url: &str,
        role: &str,
        password: &str,
    ) -> Result<PgConnection, sqlx::Error> {
        let opts = PgConnectOptions::from_str(admin_url)
            .expect("TEST_DATABASE_URL must parse as a Postgres URL")
            .username(role)
            .password(password);
        PgConnection::connect_with(&opts).await
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn ensure_app_role_is_idempotent_for_same_password() {
        let Some(url) = test_db_url() else { return };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect as admin");
        let (caller_pool, caller) = setup_caller(&admin, &url).await;
        let role = unique_role_name();
        let password = "idempotency_check_pw_42";

        ensure_app_role(&caller_pool, &role, password)
            .await
            .expect("first call");
        ensure_app_role(&caller_pool, &role, password)
            .await
            .expect("second call must not fail");

        try_login(&url, &role, password)
            .await
            .expect("login must succeed with the password after two identical calls")
            .close()
            .await
            .ok();

        caller_pool.close().await;
        cleanup_role(&admin, &role).await;
        cleanup_role(&admin, &caller).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn ensure_app_role_rotates_password_on_change() {
        let Some(url) = test_db_url() else { return };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect as admin");
        let (caller_pool, caller) = setup_caller(&admin, &url).await;
        let role = unique_role_name();
        let old_pw = "old_pw_v1_aaa";
        let new_pw = "new_pw_v2_bbb";

        ensure_app_role(&caller_pool, &role, old_pw)
            .await
            .expect("set old");
        try_login(&url, &role, old_pw)
            .await
            .expect("old password must work after first set")
            .close()
            .await
            .ok();

        ensure_app_role(&caller_pool, &role, new_pw)
            .await
            .expect("rotate");
        try_login(&url, &role, new_pw)
            .await
            .expect("new password must work after rotation")
            .close()
            .await
            .ok();

        let old_login = try_login(&url, &role, old_pw).await;
        assert!(
            old_login.is_err(),
            "old password must be rejected after rotation"
        );

        caller_pool.close().await;
        cleanup_role(&admin, &role).await;
        cleanup_role(&admin, &caller).await;
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL pointing at a PG 16+ superuser"]
    async fn ensure_app_role_handles_password_with_dollar_signs() {
        let Some(url) = test_db_url() else { return };
        let admin = PgPoolOptions::new()
            .max_connections(2)
            .connect(&url)
            .await
            .expect("connect as admin");
        let (caller_pool, caller) = setup_caller(&admin, &url).await;
        let role = unique_role_name();
        let password = r#"weird $ pw $$ with $foo$ markers and 'quotes'"#;

        ensure_app_role(&caller_pool, &role, password)
            .await
            .expect("special-char password must be set successfully");
        try_login(&url, &role, password)
            .await
            .expect("login with special-char password must succeed")
            .close()
            .await
            .ok();

        caller_pool.close().await;
        cleanup_role(&admin, &role).await;
        cleanup_role(&admin, &caller).await;
    }
}
