use postgres_protocol::password::scram_sha_256;

const VERIFIER_PREFIX: &str = "SCRAM-SHA-256$";

pub fn scram_verifier(password: &str) -> String {
    let verifier = scram_sha_256(password.as_bytes());
    assert!(
        verifier.starts_with(VERIFIER_PREFIX)
            && verifier.chars().all(
                |c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '$' | ':' | '-')
            ),
        "SCRAM verifier is not a plain verifier literal"
    );
    verifier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::names::generated_password;

    #[test]
    fn verifier_has_scram_sha_256_shape_and_hides_the_password() {
        let password = generated_password();
        let verifier = scram_verifier(&password);
        assert!(verifier.starts_with("SCRAM-SHA-256$4096:"));
        assert!(!verifier.contains(&password));
    }

    #[test]
    fn verifier_salt_is_random_per_call() {
        let password = generated_password();
        assert_ne!(scram_verifier(&password), scram_verifier(&password));
    }
}
