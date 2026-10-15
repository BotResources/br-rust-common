use uuid::Uuid;

pub fn unique_suffix() -> String {
    Uuid::now_v7().simple().to_string()[..24].to_string()
}

pub fn unique_role_name() -> String {
    format!("br_test_{}", unique_suffix())
}

pub fn unique_table_name() -> String {
    format!("br_test_tbl_{}", unique_suffix())
}

pub fn generated_password() -> String {
    format!("pw_{}", Uuid::now_v7().simple())
}

pub(crate) fn assert_plain_identifier(name: &str) {
    assert!(
        !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'),
        "identifier must match [a-z0-9_]+"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_names_are_plain_identifiers() {
        assert_plain_identifier(&unique_role_name());
        assert_plain_identifier(&unique_table_name());
    }

    #[test]
    fn generated_passwords_differ() {
        assert_ne!(generated_password(), generated_password());
    }

    #[test]
    #[should_panic(expected = "identifier must match [a-z0-9_]+")]
    fn rejects_a_quote() {
        assert_plain_identifier("a\"; DROP ROLE x;--");
    }

    #[test]
    #[should_panic(expected = "identifier must match [a-z0-9_]+")]
    fn rejects_uppercase() {
        assert_plain_identifier("Role");
    }

    #[test]
    #[should_panic(expected = "identifier must match [a-z0-9_]+")]
    fn rejects_empty() {
        assert_plain_identifier("");
    }
}
