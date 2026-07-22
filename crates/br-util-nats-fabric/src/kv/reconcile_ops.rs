use std::collections::BTreeSet;

use crate::kv::key::{KvKey, KvPrefix};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EntryAction {
    Project,
    Retract,
}

pub(super) fn decide_put<V, F: Fn(&V) -> bool>(copy_filter: &F, value: &V) -> EntryAction {
    if copy_filter(value) {
        EntryAction::Project
    } else {
        EntryAction::Retract
    }
}

pub(super) fn orphans<'a>(
    observed: &BTreeSet<KvKey>,
    desired: impl IntoIterator<Item = &'a KvKey>,
    prefixes: &[KvPrefix],
) -> Vec<KvKey> {
    let desired: BTreeSet<&KvKey> = desired.into_iter().collect();
    observed
        .iter()
        .filter(|key| prefixes.iter().any(|p| p.matches(key.as_str())))
        .filter(|key| !desired.contains(*key))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str) -> KvKey {
        KvKey::new(s).unwrap()
    }

    fn prefix(s: &str) -> KvPrefix {
        KvPrefix::new(s).unwrap()
    }

    #[test]
    fn orphans_are_observed_keys_under_a_watched_prefix_absent_from_desired() {
        let observed = BTreeSet::from([
            key("identity/users/1"),
            key("identity/users/2"),
            key("identity/users/3"),
        ]);
        let desired = [key("identity/users/2"), key("identity/users/3")];
        let prefixes = [prefix("identity/users/")];
        assert_eq!(
            orphans(&observed, desired.iter(), &prefixes),
            vec![key("identity/users/1")]
        );
    }

    #[test]
    fn orphan_detection_ignores_keys_outside_the_selected_prefixes() {
        let observed = BTreeSet::from([key("identity/groups/9")]);
        let desired: [KvKey; 0] = [];
        let prefixes = [prefix("identity/users/")];
        assert!(orphans(&observed, desired.iter(), &prefixes).is_empty());
    }

    #[derive(PartialEq, Eq)]
    struct Membership {
        active: bool,
    }

    #[test]
    fn a_passing_value_is_projected() {
        let filter = |m: &Membership| m.active;
        assert_eq!(
            decide_put(&filter, &Membership { active: true }),
            EntryAction::Project
        );
    }

    #[test]
    fn a_value_that_flips_pass_to_fail_is_retracted_locally() {
        let filter = |m: &Membership| m.active;
        assert_eq!(
            decide_put(&filter, &Membership { active: false }),
            EntryAction::Retract
        );
    }

    #[test]
    fn empty_desired_orphans_every_observed_key_under_prefix() {
        let observed = BTreeSet::from([key("identity/users/1"), key("identity/users/2")]);
        let desired: [KvKey; 0] = [];
        let prefixes = [prefix("identity/users/")];
        assert_eq!(
            orphans(&observed, desired.iter(), &prefixes),
            vec![key("identity/users/1"), key("identity/users/2")]
        );
    }
}
