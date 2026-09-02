use std::collections::BTreeSet;
use std::sync::LazyLock;

use br_core_directory::{PublishedGroup, group_id_from_kv_key, group_kv_key};
use br_util_nats_fabric::{KvKey, ProjectionSink};
use sqlx::PgConnection;
use uuid::Uuid;

use crate::consumer::recompose::member_rows;
use crate::consumer::sink::context::SinkContext;
use crate::consumer::sink::upsert::change_detecting_upsert;
use crate::error::DirectoryError;
use crate::impact::{ForeignRef, Impact};

static UPSERT: LazyLock<String> =
    LazyLock::new(|| change_detecting_upsert("known_groups", "group_id", &["name"]));

const LOCKED_GROUP: &str = "SELECT group_id FROM known_groups WHERE group_id = $1 FOR UPDATE";

const LOCKED_MEMBERS: &str =
    "SELECT user_id FROM known_user_group WHERE group_id = $1 ORDER BY user_id FOR UPDATE";

pub(crate) struct GroupSink {
    context: SinkContext,
}

impl GroupSink {
    pub(crate) fn new(context: SinkContext) -> Self {
        Self { context }
    }
}

#[async_trait::async_trait]
impl ProjectionSink<PublishedGroup> for GroupSink {
    type Error = DirectoryError;

    async fn project(&self, key: &KvKey, value: &PublishedGroup) -> Result<(), Self::Error> {
        let Some(group_id) = group_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        self.context
            .apply_in_transaction(async |conn| {
                let name_changed = sqlx::query(UPSERT.as_str())
                    .bind(group_id)
                    .bind(&value.name)
                    .execute(&mut *conn)
                    .await?
                    .rows_affected()
                    > 0;

                let projected: BTreeSet<Uuid> = member_rows(group_id, value)
                    .into_iter()
                    .map(|row| row.user_id)
                    .collect();
                let membership =
                    MembershipDiff::between(locked_members(conn, group_id).await?, projected);
                if membership.moved() {
                    membership.rewrite(conn, group_id).await?;
                }

                if !name_changed && !membership.moved() {
                    return Ok(Vec::new());
                }
                let mut impacts = vec![Impact::changed(ForeignRef::group(group_id))];
                for user_id in membership.removed {
                    impacts.push(Impact::changed(ForeignRef::user(user_id)));
                }
                Ok(impacts)
            })
            .await
    }

    async fn retract(&self, key: &KvKey) -> Result<(), Self::Error> {
        let Some(group_id) = group_id_from_kv_key(key.as_str()) else {
            return Ok(());
        };

        if !self.context.stages_impacts() {
            return self
                .context
                .apply_single_statement(async |conn| {
                    Ok(match delete_group(conn, group_id).await? {
                        true => vec![Impact::changed(ForeignRef::group(group_id))],
                        false => Vec::new(),
                    })
                })
                .await;
        }

        self.context
            .apply_in_transaction(async |conn| {
                if !locked_group(conn, group_id).await? {
                    return Ok(Vec::new());
                }
                let members = locked_members(conn, group_id).await?;
                delete_group(conn, group_id).await?;
                let mut impacts = vec![Impact::changed(ForeignRef::group(group_id))];
                for user_id in members {
                    impacts.push(Impact::changed(ForeignRef::user(user_id)));
                }
                Ok(impacts)
            })
            .await
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        let ids: Vec<(uuid::Uuid,)> = sqlx::query_as("SELECT group_id FROM known_groups")
            .fetch_all(self.context.pool())
            .await?;
        ids.into_iter()
            .map(|(id,)| KvKey::new(group_kv_key(id)).map_err(DirectoryError::from))
            .collect()
    }
}

struct MembershipDiff {
    removed: Vec<Uuid>,
    added: Vec<Uuid>,
}

impl MembershipDiff {
    fn between(current: BTreeSet<Uuid>, projected: BTreeSet<Uuid>) -> Self {
        Self {
            removed: current.difference(&projected).copied().collect(),
            added: projected.difference(&current).copied().collect(),
        }
    }

    fn moved(&self) -> bool {
        !self.removed.is_empty() || !self.added.is_empty()
    }

    async fn rewrite(&self, conn: &mut PgConnection, group_id: Uuid) -> Result<(), DirectoryError> {
        if !self.removed.is_empty() {
            sqlx::query(
                "DELETE FROM known_user_group WHERE group_id = $1 AND user_id = ANY($2::uuid[])",
            )
            .bind(group_id)
            .bind(&self.removed)
            .execute(&mut *conn)
            .await?;
        }
        if !self.added.is_empty() {
            sqlx::query(
                "INSERT INTO known_user_group (group_id, user_id) \
                 SELECT $1, unnest($2::uuid[]) \
                 ON CONFLICT (group_id, user_id) DO NOTHING",
            )
            .bind(group_id)
            .bind(&self.added)
            .execute(&mut *conn)
            .await?;
        }
        Ok(())
    }
}

async fn locked_group(conn: &mut PgConnection, group_id: Uuid) -> Result<bool, DirectoryError> {
    let row: Option<(Uuid,)> = sqlx::query_as(LOCKED_GROUP)
        .bind(group_id)
        .fetch_optional(conn)
        .await?;
    Ok(row.is_some())
}

async fn delete_group(conn: &mut PgConnection, group_id: Uuid) -> Result<bool, DirectoryError> {
    let deleted = sqlx::query("DELETE FROM known_groups WHERE group_id = $1")
        .bind(group_id)
        .execute(conn)
        .await?
        .rows_affected();
    Ok(deleted > 0)
}

async fn locked_members(
    conn: &mut PgConnection,
    group_id: Uuid,
) -> Result<BTreeSet<Uuid>, DirectoryError> {
    let rows: Vec<(Uuid,)> = sqlx::query_as(LOCKED_MEMBERS)
        .bind(group_id)
        .fetch_all(conn)
        .await?;
    Ok(rows.into_iter().map(|(user_id,)| user_id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u128) -> Uuid {
        Uuid::from_u128(n)
    }

    fn set(ns: &[u128]) -> BTreeSet<Uuid> {
        ns.iter().map(|n| id(*n)).collect()
    }

    fn published(member_ns: &[u128]) -> PublishedGroup {
        PublishedGroup::new(
            "crew".to_string(),
            member_ns.iter().map(|n| id(*n)).collect(),
            std::collections::BTreeMap::new(),
        )
        .expect("a published group")
    }

    fn projected(member_ns: &[u128]) -> BTreeSet<Uuid> {
        member_rows(id(100), &published(member_ns))
            .into_iter()
            .map(|row| row.user_id)
            .collect()
    }

    #[test]
    fn a_member_set_is_compared_as_a_set_not_as_a_published_sequence() {
        let canonical = projected(&[1, 2]);
        let permuted_with_duplicates = projected(&[2, 1, 2]);
        assert_eq!(canonical, permuted_with_duplicates);

        let diff = MembershipDiff::between(canonical, permuted_with_duplicates);
        assert!(!diff.moved());
        assert!(diff.removed.is_empty());
        assert!(diff.added.is_empty());
    }

    #[test]
    fn a_dropped_member_is_reported_as_removed_and_names_itself() {
        let diff = MembershipDiff::between(set(&[1, 2]), set(&[1]));
        assert!(diff.moved());
        assert_eq!(diff.removed, vec![id(2)]);
        assert!(diff.added.is_empty());
    }

    #[test]
    fn a_new_member_is_reported_as_added_only() {
        let diff = MembershipDiff::between(set(&[1]), set(&[1, 3]));
        assert!(diff.moved());
        assert_eq!(diff.added, vec![id(3)]);
        assert!(diff.removed.is_empty());
    }
}
