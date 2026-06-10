use sqlx::PgPool;
use uuid::Uuid;

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
/// 3. Runs `ALTER ROLE "<name>" PASSWORD $tag$<password>$tag$` with the
///    password embedded as a dollar-quoted literal. Postgres rejects bind
///    parameters in DDL (`syntax error at or near "$1"`), so the secret
///    necessarily appears in the SQL text — dollar-quoting with a per-call
///    random tag (`br_<uuid-v7-simple>`) gives byte-exact passthrough with
///    no escape rules to mishandle, and the unguessable tag means a
///    malicious password cannot break out of the literal. Setting the
///    password only needs membership in the target role, which the
///    CREATEROLE creator receives implicitly. The SQL is never logged or
///    surfaced in errors so the secret stays out of traces.
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

    let tag = fresh_dollar_quote_tag();
    let alter_sql = build_alter_password_sql(role_name, password, &tag);
    // Deliberately do not include `alter_sql` in any tracing event or error:
    // it contains the plaintext password as a dollar-quoted literal.
    let result = sqlx::query(&alter_sql).execute(pool).await;
    // Zero the SQL string promptly so the password does not linger in memory
    // longer than necessary. This is best-effort — sqlx has already copied
    // the bytes onto the wire — but it shortens the window for an
    // accidental dump (panic backtrace formatter, allocator reuse, etc).
    drop(scrub(alter_sql));
    result.map_err(PostgresError::Db)?;

    Ok(())
}

/// Build the `ALTER ROLE ... PASSWORD` DDL with the password as a
/// dollar-quoted literal. Caller supplies a fresh, unguessable `tag`
/// — `ensure_app_role` uses one UUID v7 per invocation.
///
/// Panics:
/// - If `role_name` is not pre-validated (must match `^[a-z][a-z0-9_]*$`).
/// - If `tag` is not a valid Postgres identifier.
/// - If `password` happens to contain the closing delimiter `$tag$`. With
///   a fresh UUID v7 the probability is ~2⁻¹²⁸ per call — far below any
///   realistic-error threshold, so panicking is the right outcome
///   (surfaces an RNG / cosmic-ray issue loudly rather than emitting
///   injectable DDL). The panic message does **not** include the
///   password.
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
    // Defense-in-depth: if the password ever contains the random closing
    // delimiter it would break out of the literal. Both ends are
    // controlled by us (the password is internally generated, the tag is
    // a fresh UUID v7), so this branch is unreachable in practice — we
    // panic instead of returning an error to avoid growing the public
    // error surface for a cryptographically-impossible case.
    assert!(
        !password.contains(&delimiter),
        "fresh UUID-v7 tag collided with password content — cryptographically impossible; \
         check your RNG"
    );
    format!("ALTER ROLE \"{role_name}\" PASSWORD {delimiter}{password}{delimiter}")
}

/// Per-call dollar-quote tag of the form `br_<32-hex>`. UUID v7's time-
/// ordered + random suffix is unguessable enough for break-out defense
/// and the `.simple()` rendering (no hyphens) is a valid Postgres
/// identifier without quoting.
fn fresh_dollar_quote_tag() -> String {
    format!("br_{}", Uuid::now_v7().simple())
}

fn is_valid_dollar_quote_tag(tag: &str) -> bool {
    // Postgres dollar-quote tags must match identifier rules:
    // first char letter or underscore, rest alphanumeric or underscore.
    let mut chars = tag.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Overwrite the string's bytes with zeros before dropping. Best effort:
/// the allocator may have already copied the buffer, and the password
/// has likely already been written to the socket. The intent is just to
/// shorten the residency window in our own process memory.
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

    // ─── build_alter_password_sql ────────────────────

    const TEST_TAG: &str = "br_test";

    #[test]
    fn builds_dollar_quoted_password_literal() {
        let sql = build_alter_password_sql("app", "s3cret", TEST_TAG);
        assert_eq!(sql, "ALTER ROLE \"app\" PASSWORD $br_test$s3cret$br_test$");
    }

    #[test]
    fn does_not_emit_bind_parameters() {
        // The whole point of the 0.5.2 fix: Postgres rejects $1 in DDL.
        let sql = build_alter_password_sql("app", "hunter2", TEST_TAG);
        assert!(
            !sql.contains("$1"),
            "SQL must not contain a bind placeholder: {sql}"
        );
    }

    #[test]
    fn passes_special_characters_through_verbatim() {
        // Dollar-quoting means no escape rules: quotes, backslashes, and
        // even literal `$` in the password go through unchanged as long as
        // they don't form the closing delimiter.
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
        // Defense-in-depth: if the password would break out of the literal,
        // panic rather than emit malformed/injectable SQL. The panic message
        // must not echo the password content — see the `expected = ...`
        // matcher above (which does NOT include "DROP TABLE").
        build_alter_password_sql("app", "anything$br_test$DROP TABLE", TEST_TAG);
    }

    #[test]
    #[should_panic(expected = "role_name must be pre-validated")]
    fn panics_on_unvalidated_role_name() {
        // Internal helper: callers must run validate_role_name first.
        build_alter_password_sql("bad-name", "pw", TEST_TAG);
    }

    // ─── fresh_dollar_quote_tag ──────────────────────

    #[test]
    fn fresh_tag_is_a_valid_postgres_identifier() {
        let tag = fresh_dollar_quote_tag();
        assert!(tag.starts_with("br_"));
        assert!(is_valid_dollar_quote_tag(&tag), "tag={tag}");
    }

    #[test]
    fn fresh_tag_differs_between_calls() {
        // Random per call so a single-process attacker who has seen one
        // tag cannot predict the next. UUID v7 has a millisecond-resolution
        // timestamp plus random bits; back-to-back calls differ in the
        // random portion.
        let a = fresh_dollar_quote_tag();
        let b = fresh_dollar_quote_tag();
        assert_ne!(a, b);
    }

    // ─── is_valid_dollar_quote_tag ───────────────────

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

/// Live-Postgres tests for `ensure_app_role`.
///
/// Run in CI under the `e2e-postgres` job against a `postgres:16-alpine`
/// service, and locally with `TEST_DATABASE_URL` pointing at any PG 16+
/// superuser connection — invoke explicitly via `cargo test -- --ignored`.
///
/// Each test bootstraps a fresh `caller_<uuid>` role configured exactly
/// like CNPG's `<svc>_owner` in production (`LOGIN CREATEROLE NOSUPERUSER`,
/// no other attributes), opens a pool **as that caller**, and passes that
/// pool to `ensure_app_role`. Calling through a SUPERUSER admin pool would
/// hide the Scenario 1 regression from issue #13 — PG 16+ rejects
/// `NOSUPERUSER` / `NOBYPASSRLS` / `NOREPLICATION` assertions from
/// non-superuser CREATEROLE callers even when value-equivalent, and only
/// the non-superuser caller path can catch a re-introduction of that bug.
///
/// They exercise the two behavioral guarantees that broke between 0.5.0
/// and 0.5.1: that the call succeeds end-to-end against the production
/// privilege model (no `permission denied to alter role`, no
/// `syntax error at or near "$1"`), and that calling twice with different
/// passwords actually rotates the secret on the server.
#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::test_support::{cleanup_role, setup_caller, test_db_url, unique_role_name};
    use sqlx::Connection;
    use sqlx::postgres::{PgConnectOptions, PgConnection, PgPoolOptions};
    use std::str::FromStr;

    /// Connect to the same cluster as `admin_url` but as `role`/`password`.
    /// Returns the connection on success, the sqlx error on failure (we use
    /// the error to assert that an *old* password no longer authenticates).
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
        // Drop the app role before the caller — the caller owns it (it
        // created it via CREATEROLE) and PG refuses to drop a role that
        // still owns other roles.
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
        // 0.5.0/0.5.1 never got far enough to exercise this — but with
        // dollar-quoting in 0.5.2 a password containing `$` characters
        // must still pass through verbatim and authenticate.
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
