pub(crate) fn assert_lower_snake_reason(reason_code: &str) {
    assert!(
        is_lower_snake(reason_code),
        "reason code must be lower_snake_case (FE i18n key suffix for affordance and edge-error reasons), got: {reason_code:?}"
    );
}

fn is_lower_snake(candidate: &str) -> bool {
    let mut chars = candidate.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    candidate
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_lower_snake_codes() {
        for code in [
            "title_generating",
            "name_already_taken",
            "seat_limit_reached",
            "cooldown_active",
            "locked",
            "retry_after_30s",
        ] {
            assert!(is_lower_snake(code), "should accept {code:?}");
        }
    }

    #[test]
    fn rejects_screaming_snake() {
        assert!(!is_lower_snake("NOT_COLLEGE_MEMBER"));
        assert!(!is_lower_snake("PROPOSAL_CLOSED"));
        assert!(!is_lower_snake("Name_Already_Taken"));
    }

    #[test]
    fn rejects_kebab_case() {
        assert!(!is_lower_snake("name-already-taken"));
    }

    #[test]
    fn rejects_spaces() {
        assert!(!is_lower_snake("name already taken"));
    }

    #[test]
    fn rejects_empty() {
        assert!(!is_lower_snake(""));
    }

    #[test]
    fn rejects_leading_underscore_or_digit() {
        assert!(!is_lower_snake("_leading"));
        assert!(!is_lower_snake("1_leading"));
    }

    #[test]
    #[should_panic(expected = "lower_snake_case")]
    fn assert_panics_on_screaming() {
        assert_lower_snake_reason("SCREAMING_CODE");
    }

    #[test]
    fn assert_accepts_snake() {
        assert_lower_snake_reason("name_already_taken");
    }
}
