use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KvOp<K, V> {
    Put { key: K, value: V },
    Delete { key: K },
}

pub fn reconcile<K, V>(desired: &BTreeMap<K, V>, observed: &BTreeMap<K, V>) -> Vec<KvOp<K, V>>
where
    K: Ord + Clone,
    V: PartialEq + Clone,
{
    let mut ops = Vec::new();

    for (key, value) in desired {
        match observed.get(key) {
            Some(current) if current == value => {}
            _ => ops.push(KvOp::Put {
                key: key.clone(),
                value: value.clone(),
            }),
        }
    }

    for key in observed.keys() {
        if !desired.contains_key(key) {
            ops.push(KvOp::Delete { key: key.clone() });
        }
    }

    ops
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn empty_observed_puts_everything_desired() {
        let ops = reconcile(&map(&[("a", "1"), ("b", "2")]), &map(&[]));
        assert_eq!(
            ops,
            vec![
                KvOp::Put {
                    key: "a".to_string(),
                    value: "1".to_string()
                },
                KvOp::Put {
                    key: "b".to_string(),
                    value: "2".to_string()
                },
            ]
        );
    }

    #[test]
    fn unchanged_entry_yields_no_op() {
        assert!(reconcile(&map(&[("a", "1")]), &map(&[("a", "1")])).is_empty());
    }

    #[test]
    fn changed_entry_is_a_put() {
        let ops = reconcile(&map(&[("a", "new")]), &map(&[("a", "old")]));
        assert_eq!(
            ops,
            vec![KvOp::Put {
                key: "a".to_string(),
                value: "new".to_string()
            }]
        );
    }

    #[test]
    fn observed_orphan_is_deleted() {
        let ops = reconcile(&map(&[]), &map(&[("gone", "x")]));
        assert_eq!(
            ops,
            vec![KvOp::Delete {
                key: "gone".to_string()
            }]
        );
    }

    #[test]
    fn mixed_diff_puts_changed_then_deletes_orphans() {
        let desired = map(&[("a", "1"), ("b", "b2"), ("c", "c")]);
        let observed = map(&[("b", "b1"), ("c", "c"), ("z", "orphan")]);
        let ops = reconcile(&desired, &observed);
        assert_eq!(
            ops,
            vec![
                KvOp::Put {
                    key: "a".to_string(),
                    value: "1".to_string()
                },
                KvOp::Put {
                    key: "b".to_string(),
                    value: "b2".to_string()
                },
                KvOp::Delete {
                    key: "z".to_string()
                },
            ]
        );
    }
}
