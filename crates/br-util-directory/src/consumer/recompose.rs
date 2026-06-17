use br_core_directory::PublishedGroup;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberRow {
    pub group_id: Uuid,
    pub user_id: Uuid,
}

pub fn member_rows(group_id: Uuid, group: &PublishedGroup) -> Vec<MemberRow> {
    group
        .member_ids
        .iter()
        .map(|user_id| MemberRow {
            group_id,
            user_id: *user_id,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn group(member_ns: &[u128]) -> PublishedGroup {
        PublishedGroup::new(
            "engineering".to_string(),
            member_ns.iter().map(|n| id(*n)).collect(),
            BTreeMap::new(),
        )
        .unwrap()
    }

    #[test]
    fn denormalized_member_ids_become_one_junction_row_each() {
        let rows = member_rows(id(100), &group(&[1, 2, 3]));
        assert_eq!(
            rows,
            vec![
                MemberRow {
                    group_id: id(100),
                    user_id: id(1)
                },
                MemberRow {
                    group_id: id(100),
                    user_id: id(2)
                },
                MemberRow {
                    group_id: id(100),
                    user_id: id(3)
                },
            ]
        );
    }

    #[test]
    fn empty_group_recomposes_to_no_junction_rows() {
        assert!(member_rows(id(100), &group(&[])).is_empty());
    }

    #[test]
    fn every_row_carries_the_group_id_from_the_kv_key() {
        let rows = member_rows(id(42), &group(&[7]));
        assert_eq!(rows[0].group_id, id(42));
        assert_eq!(rows[0].user_id, id(7));
    }
}
