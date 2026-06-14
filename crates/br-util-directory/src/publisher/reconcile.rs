use std::collections::BTreeMap;

use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvOp<T> {
    Put { id: Uuid, value: T },
    Delete { id: Uuid },
}

pub fn reconcile_entries<T: PartialEq + Clone>(
    desired: &BTreeMap<Uuid, T>,
    observed: &BTreeMap<Uuid, T>,
) -> Vec<KvOp<T>> {
    let mut ops = Vec::new();

    for (id, value) in desired {
        match observed.get(id) {
            Some(current) if current == value => {}
            _ => ops.push(KvOp::Put {
                id: *id,
                value: value.clone(),
            }),
        }
    }

    for id in observed.keys() {
        if !desired.contains_key(id) {
            ops.push(KvOp::Delete { id: *id });
        }
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn map(pairs: &[(u128, &str)]) -> BTreeMap<Uuid, String> {
        pairs.iter().map(|(n, v)| (id(*n), v.to_string())).collect()
    }

    #[test]
    fn empty_observed_puts_everything_desired() {
        let ops = reconcile_entries(&map(&[(1, "a"), (2, "b")]), &map(&[]));
        assert_eq!(
            ops,
            vec![
                KvOp::Put {
                    id: id(1),
                    value: "a".to_string()
                },
                KvOp::Put {
                    id: id(2),
                    value: "b".to_string()
                },
            ]
        );
    }

    #[test]
    fn unchanged_entry_yields_no_op() {
        let ops = reconcile_entries(&map(&[(1, "a")]), &map(&[(1, "a")]));
        assert!(ops.is_empty());
    }

    #[test]
    fn changed_entry_is_a_put() {
        let ops = reconcile_entries(&map(&[(1, "new")]), &map(&[(1, "old")]));
        assert_eq!(
            ops,
            vec![KvOp::Put {
                id: id(1),
                value: "new".to_string()
            }]
        );
    }

    #[test]
    fn observed_orphan_is_deleted_for_pii_propagation() {
        let ops = reconcile_entries(&map(&[]), &map(&[(7, "gone")]));
        assert_eq!(ops, vec![KvOp::Delete { id: id(7) }]);
    }

    #[test]
    fn mixed_diff_puts_changed_and_new_then_deletes_orphans() {
        let desired = map(&[(1, "a"), (2, "b2"), (3, "c")]);
        let observed = map(&[(2, "b1"), (3, "c"), (9, "orphan")]);
        let ops = reconcile_entries(&desired, &observed);
        assert_eq!(
            ops,
            vec![
                KvOp::Put {
                    id: id(1),
                    value: "a".to_string()
                },
                KvOp::Put {
                    id: id(2),
                    value: "b2".to_string()
                },
                KvOp::Delete { id: id(9) },
            ]
        );
    }
}
