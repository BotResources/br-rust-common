use postgres_protocol::password::scram_sha_256;

use crate::error::PostgresError;

const VERIFIER_PREFIX: &str = "SCRAM-SHA-256$";

pub(crate) fn scram_verifier(password: &str) -> Result<String, PostgresError> {
    if password.is_empty() {
        return Err(PostgresError::Config(
            "role password must not be empty".to_string(),
        ));
    }
    if password.contains('\0') {
        return Err(PostgresError::Config(
            "role password must not contain a NUL byte".to_string(),
        ));
    }
    let verifier = scram_sha_256(password.as_bytes());
    if !is_safe_verifier(&verifier) {
        return Err(PostgresError::Config(
            "generated SCRAM verifier is not a plain verifier literal".to_string(),
        ));
    }
    Ok(verifier)
}

fn is_safe_verifier(verifier: &str) -> bool {
    verifier.starts_with(VERIFIER_PREFIX)
        && verifier
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '$' | ':' | '-'))
}

pub(crate) fn alter_role_verifier_sql(
    role_name: &str,
    verifier: &str,
) -> Result<String, PostgresError> {
    crate::role::validate_role_name(role_name)?;
    if !is_safe_verifier(verifier) {
        return Err(PostgresError::Config(
            "SCRAM verifier contains characters outside the verifier alphabet".to_string(),
        ));
    }
    Ok(format!("ALTER ROLE \"{role_name}\" PASSWORD '{verifier}'"))
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;

    fn generated_password() -> String {
        format!("pw_{}", Uuid::now_v7().simple())
    }

    #[test]
    fn verifier_has_scram_sha_256_shape_with_4096_iterations() {
        let verifier = scram_verifier(&generated_password()).expect("verifier");
        let parts: Vec<&str> = verifier.split(['$', ':']).collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(parts[0], "SCRAM-SHA-256");
        assert_eq!(parts[1], "4096");
    }

    #[test]
    fn verifier_never_contains_the_clear_password() {
        let password = generated_password();
        let verifier = scram_verifier(&password).expect("verifier");
        assert!(!verifier.contains(&password));
    }

    #[test]
    fn verifier_salt_is_random_per_call() {
        let password = generated_password();
        assert_ne!(
            scram_verifier(&password).expect("first"),
            scram_verifier(&password).expect("second")
        );
    }

    #[test]
    fn rejects_empty_password() {
        let empty = String::new();
        assert!(matches!(
            scram_verifier(&empty),
            Err(PostgresError::Config(_))
        ));
    }

    #[test]
    fn rejects_password_with_a_nul_byte() {
        let password = format!("{}\0{}", generated_password(), generated_password());
        assert!(matches!(
            scram_verifier(&password),
            Err(PostgresError::Config(_))
        ));
    }

    #[test]
    fn statement_text_contains_the_verifier_and_no_clear_password() {
        let password = generated_password();
        let verifier = scram_verifier(&password).expect("verifier");
        let sql = alter_role_verifier_sql("app", &verifier).expect("sql");
        assert_eq!(sql, format!("ALTER ROLE \"app\" PASSWORD '{verifier}'"));
        assert!(!sql.contains(&password));
    }

    #[test]
    fn statement_for_password_with_quotes_and_dollars_has_no_clear_text() {
        let password = format!(r#"'"\;-- $$ $foo$ {}"#, Uuid::now_v7().simple());
        let verifier = scram_verifier(&password).expect("verifier");
        let sql = alter_role_verifier_sql("app", &verifier).expect("sql");
        assert!(!sql.contains(&password));
        assert_eq!(sql.matches('\'').count(), 2);
    }

    #[test]
    fn rejects_verifier_with_a_quote() {
        let result = alter_role_verifier_sql("app", "SCRAM-SHA-256$4096:aa$bb:cc'; DROP ROLE x;--");
        assert!(matches!(result, Err(PostgresError::Config(_))));
    }

    #[test]
    fn rejects_verifier_with_whitespace_or_backslash() {
        for bad in ["SCRAM-SHA-256$4096:a a$b:c", "SCRAM-SHA-256$4096:a\\a$b:c"] {
            assert!(
                matches!(
                    alter_role_verifier_sql("app", bad),
                    Err(PostgresError::Config(_))
                ),
                "bad={bad}"
            );
        }
    }

    #[test]
    fn rejects_text_that_is_not_a_scram_verifier() {
        let result = alter_role_verifier_sql("app", &generated_password());
        assert!(matches!(result, Err(PostgresError::Config(_))));
    }

    #[test]
    fn rejects_unvalidated_role_name() {
        let verifier = scram_verifier(&generated_password()).expect("verifier");
        assert!(matches!(
            alter_role_verifier_sql("bad-name", &verifier),
            Err(PostgresError::InvalidRoleName(_))
        ));
    }
}
