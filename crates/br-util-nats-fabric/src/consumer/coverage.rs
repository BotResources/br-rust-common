pub(crate) fn subject_covered(configured: &[String], subject: &str) -> bool {
    configured
        .iter()
        .any(|pattern| pattern_covers(pattern, subject))
}

fn pattern_covers(pattern: &str, subject: &str) -> bool {
    if pattern.is_empty() || subject.is_empty() {
        return false;
    }

    let mut subject_tokens = subject.split('.');
    let mut pattern_tokens = pattern.split('.');

    while let Some(pattern_token) = pattern_tokens.next() {
        match pattern_token {
            ">" => return pattern_tokens.next().is_none() && subject_tokens.next().is_some(),
            "*" => {
                if subject_tokens.next().is_none() {
                    return false;
                }
            }
            literal => match subject_tokens.next() {
                Some(subject_token) if subject_token == literal => {}
                _ => return false,
            },
        }
    }

    subject_tokens.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn configured(patterns: &[&str]) -> Vec<String> {
        patterns.iter().map(|p| (*p).to_string()).collect()
    }

    fn covers(pattern: &str, subject: &str) -> bool {
        subject_covered(&configured(&[pattern]), subject)
    }

    #[test]
    fn a_tail_wildcard_covers_a_deeper_coordinate() {
        assert!(covers("integration.evt.>", "integration.evt.a.b.c.v1"));
    }

    #[test]
    fn a_single_token_wildcard_does_not_cover_a_deeper_coordinate() {
        assert!(!covers("integration.evt.*", "integration.evt.a.b.c.v1"));
    }

    #[test]
    fn a_single_token_wildcard_covers_exactly_one_token() {
        assert!(covers("integration.evt.*", "integration.evt.identity"));
    }

    #[test]
    fn the_command_binding_does_not_cover_an_event_subject() {
        assert!(!covers("integration.cmd.>", "integration.evt.a.b.c.v1"));
    }

    #[test]
    fn an_empty_configured_list_covers_nothing() {
        assert!(!subject_covered(&[], "integration.evt.a.b.c.v1"));
    }

    #[test]
    fn a_lone_tail_wildcard_covers_everything() {
        assert!(covers(">", "integration.evt.a.b.c.v1"));
        assert!(covers(">", "anything"));
    }

    #[test]
    fn a_trailing_tail_wildcard_needs_at_least_one_token() {
        assert!(!covers("integration.evt.>", "integration.evt"));
        assert!(covers("integration.evt.>", "integration.evt.a"));
    }

    #[test]
    fn a_literal_pattern_covers_only_the_exact_subject() {
        assert!(covers(
            "integration.evt.identity.user.created.v1",
            "integration.evt.identity.user.created.v1"
        ));
        assert!(!covers(
            "integration.evt.identity.user.created.v1",
            "integration.evt.identity.user.created.v2"
        ));
    }

    #[test]
    fn a_shorter_literal_pattern_does_not_cover_a_longer_subject() {
        assert!(!covers("integration.evt", "integration.evt.a.b.c.v1"));
    }

    #[test]
    fn a_longer_pattern_does_not_cover_a_shorter_subject() {
        assert!(!covers("integration.evt.a.b", "integration.evt.a"));
    }

    #[test]
    fn any_configured_pattern_matching_is_enough() {
        let subjects = configured(&["integration.cmd.>", "integration.evt.>"]);
        assert!(subject_covered(&subjects, "integration.evt.a.b.c.v1"));
        assert!(subject_covered(&subjects, "integration.cmd.a.b.c.v1"));
        assert!(!subject_covered(&subjects, "other.a.b.c.v1"));
    }

    #[test]
    fn an_interior_tail_wildcard_is_not_a_match() {
        assert!(!covers("integration.>.v1", "integration.evt.v1"));
    }

    #[test]
    fn an_empty_subject_is_never_covered() {
        assert!(!subject_covered(&configured(&[">"]), ""));
    }

    #[test]
    fn an_empty_pattern_covers_nothing() {
        assert!(!covers("", "integration.evt.a.b.c.v1"));
    }

    #[test]
    fn a_mid_pattern_single_wildcard_matches_one_token() {
        assert!(covers(
            "integration.evt.*.user.created.v1",
            "integration.evt.identity.user.created.v1"
        ));
        assert!(!covers(
            "integration.evt.*.user.created.v1",
            "integration.evt.identity.group.created.v1"
        ));
    }

    #[test]
    fn a_wildcard_prefix_with_a_tail_covers_the_rest() {
        assert!(covers(
            "integration.*.identity.>",
            "integration.evt.identity.user.created.v1"
        ));
    }
}
