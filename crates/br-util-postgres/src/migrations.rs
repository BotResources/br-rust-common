use std::collections::{HashMap, HashSet};

use sqlx::PgPool;
use sqlx::migrate::{AppliedMigration, Migrate, MigrateError, Migration, Migrator};

use crate::error::PostgresError;

const UNDEFINED_TABLE: &str = "42P01";

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MigrationsStatus {
    pub applied: usize,
    pub pending: Vec<i64>,
    pub checksum_mismatch: Vec<i64>,
    pub applied_not_embedded: Vec<i64>,
    pub dirty: Option<i64>,
}

impl MigrationsStatus {
    pub fn is_current(&self) -> bool {
        self.embedded_applied() && self.applied_not_embedded.is_empty()
    }

    pub fn embedded_applied(&self) -> bool {
        self.pending.is_empty() && self.checksum_mismatch.is_empty() && self.dirty.is_none()
    }
}

pub async fn migrations_status(
    pool: &PgPool,
    migrator: &Migrator,
) -> Result<MigrationsStatus, PostgresError> {
    let mut conn = pool.acquire().await.map_err(PostgresError::Db)?;

    let applied = match conn.list_applied_migrations().await {
        Ok(applied) => applied,
        Err(e) if is_missing_migrations_table(&e) => {
            return Ok(compare(migrator.iter(), &[], None));
        }
        Err(e) => return Err(as_postgres_error(e)),
    };

    let dirty = conn.dirty_version().await.map_err(as_postgres_error)?;

    Ok(compare(migrator.iter(), &applied, dirty))
}

fn is_missing_migrations_table(error: &MigrateError) -> bool {
    matches!(
        error,
        MigrateError::Execute(sqlx::Error::Database(db))
            if db.code().as_deref() == Some(UNDEFINED_TABLE)
    )
}

fn as_postgres_error(error: MigrateError) -> PostgresError {
    PostgresError::Db(sqlx::Error::from(error))
}

fn compare<'a>(
    embedded: impl Iterator<Item = &'a Migration>,
    applied: &[AppliedMigration],
    dirty: Option<i64>,
) -> MigrationsStatus {
    let rows: HashMap<i64, &[u8]> = applied
        .iter()
        .map(|m| (m.version, m.checksum.as_ref()))
        .collect();

    let mut embedded_versions: HashSet<i64> = HashSet::new();
    let mut applied_count = 0usize;
    let mut pending = Vec::new();
    let mut checksum_mismatch = Vec::new();

    for migration in embedded {
        embedded_versions.insert(migration.version);

        if migration.migration_type.is_down_migration() {
            continue;
        }

        match rows.get(&migration.version) {
            None => pending.push(migration.version),
            Some(checksum) => {
                applied_count += 1;
                if *checksum != migration.checksum.as_ref() {
                    checksum_mismatch.push(migration.version);
                }
            }
        }
    }

    let mut applied_not_embedded: Vec<i64> = rows
        .into_keys()
        .filter(|version| !embedded_versions.contains(version))
        .collect();

    pending.sort_unstable();
    checksum_mismatch.sort_unstable();
    applied_not_embedded.sort_unstable();

    MigrationsStatus {
        applied: applied_count,
        pending,
        checksum_mismatch,
        applied_not_embedded,
        dirty,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::migrate::MigrationType;
    use std::borrow::Cow;

    fn migration(version: i64, sql: &'static str) -> Migration {
        Migration::new(
            version,
            Cow::Borrowed("fixture"),
            MigrationType::Simple,
            Cow::Borrowed(sql),
            false,
        )
    }

    fn reversible(version: i64, up_sql: &'static str, down_sql: &'static str) -> [Migration; 2] {
        [
            Migration::new(
                version,
                Cow::Borrowed("fixture"),
                MigrationType::ReversibleUp,
                Cow::Borrowed(up_sql),
                false,
            ),
            Migration::new(
                version,
                Cow::Borrowed("fixture"),
                MigrationType::ReversibleDown,
                Cow::Borrowed(down_sql),
                false,
            ),
        ]
    }

    fn applied(migration: &Migration) -> AppliedMigration {
        AppliedMigration {
            version: migration.version,
            checksum: migration.checksum.clone(),
        }
    }

    fn tampered(migration: &Migration) -> AppliedMigration {
        AppliedMigration {
            version: migration.version,
            checksum: Cow::Owned(vec![0u8; 48]),
        }
    }

    #[test]
    fn empty_database_leaves_every_embedded_migration_pending() {
        let embedded = [migration(1, "SELECT 1"), migration(2, "SELECT 2")];

        let status = compare(embedded.iter(), &[], None);

        assert_eq!(status.applied, 0);
        assert_eq!(status.pending, vec![1, 2]);
        assert!(!status.is_current());
    }

    #[test]
    fn fully_applied_set_is_current() {
        let embedded = [migration(1, "SELECT 1"), migration(2, "SELECT 2")];
        let rows: Vec<AppliedMigration> = embedded.iter().map(applied).collect();

        let status = compare(embedded.iter(), &rows, None);

        assert_eq!(status.applied, 2);
        assert!(status.pending.is_empty());
        assert!(status.checksum_mismatch.is_empty());
        assert!(status.applied_not_embedded.is_empty());
        assert!(status.embedded_applied());
        assert!(status.is_current());
    }

    #[test]
    fn missing_tail_is_pending() {
        let embedded = [migration(1, "SELECT 1"), migration(2, "SELECT 2")];
        let rows = vec![applied(&embedded[0])];

        let status = compare(embedded.iter(), &rows, None);

        assert_eq!(status.applied, 1);
        assert_eq!(status.pending, vec![2]);
        assert!(!status.is_current());
    }

    #[test]
    fn edited_migration_is_a_checksum_mismatch_not_pending() {
        let embedded = [migration(1, "SELECT 1"), migration(2, "SELECT 2")];
        let rows = vec![applied(&embedded[0]), tampered(&embedded[1])];

        let status = compare(embedded.iter(), &rows, None);

        assert_eq!(status.applied, 2);
        assert!(status.pending.is_empty());
        assert_eq!(status.checksum_mismatch, vec![2]);
        assert!(!status.is_current());
    }

    #[test]
    fn database_ahead_of_the_binary_is_not_pending_and_not_current() {
        let embedded = [migration(1, "SELECT 1")];
        let ahead = migration(9, "SELECT 9");
        let rows = vec![applied(&embedded[0]), applied(&ahead)];

        let status = compare(embedded.iter(), &rows, None);

        assert_eq!(status.applied, 1);
        assert!(status.pending.is_empty());
        assert_eq!(status.applied_not_embedded, vec![9]);
        assert!(status.embedded_applied());
        assert!(!status.is_current());
    }

    #[test]
    fn dirty_version_defeats_both_predicates() {
        let embedded = [migration(1, "SELECT 1")];
        let rows: Vec<AppliedMigration> = embedded.iter().map(applied).collect();

        let status = compare(embedded.iter(), &rows, Some(1));

        assert_eq!(status.dirty, Some(1));
        assert!(!status.embedded_applied());
        assert!(!status.is_current());
    }

    #[test]
    fn down_migrations_are_never_pending() {
        let pair = reversible(1, "CREATE TABLE widget ()", "DROP TABLE widget");

        let status = compare(pair.iter(), &[], None);

        assert_eq!(status.applied, 0);
        assert_eq!(status.pending, vec![1]);
        assert!(!status.is_current());
    }

    #[test]
    fn fully_applied_reversible_set_is_current() {
        let pair = reversible(1, "CREATE TABLE widget ()", "DROP TABLE widget");
        let rows = vec![applied(&pair[0])];

        let status = compare(pair.iter(), &rows, None);

        assert_eq!(status.applied, 1);
        assert!(status.pending.is_empty());
        assert!(status.checksum_mismatch.is_empty());
        assert!(status.applied_not_embedded.is_empty());
        assert!(status.is_current());
    }

    #[test]
    fn fully_applied_reversible_set_with_a_database_ahead_is_not_current() {
        let pair = reversible(1, "CREATE TABLE widget ()", "DROP TABLE widget");
        let ahead = migration(9, "SELECT 9");
        let rows = vec![applied(&pair[0]), applied(&ahead)];

        let status = compare(pair.iter(), &rows, None);

        assert!(status.pending.is_empty());
        assert_eq!(status.applied_not_embedded, vec![9]);
        assert!(status.embedded_applied());
        assert!(!status.is_current());
    }

    #[test]
    fn a_version_carried_only_by_a_down_entry_still_counts_as_embedded() {
        let pair = reversible(1, "CREATE TABLE widget ()", "DROP TABLE widget");
        let orphan_down = &pair[1];
        let rows = vec![applied(orphan_down)];

        let status = compare(std::iter::once(orphan_down), &rows, None);

        assert!(status.applied_not_embedded.is_empty());
        assert!(status.pending.is_empty());
        assert!(status.checksum_mismatch.is_empty());
        assert!(status.is_current());
    }

    #[test]
    fn checksum_is_compared_against_the_up_migration() {
        let pair = reversible(1, "CREATE TABLE widget ()", "DROP TABLE widget");
        let rows = vec![applied(&pair[1])];

        let status = compare(pair.iter(), &rows, None);

        assert_eq!(status.applied, 1);
        assert!(status.pending.is_empty());
        assert_eq!(status.checksum_mismatch, vec![1]);
        assert!(!status.is_current());
    }

    #[test]
    fn a_down_migration_listed_first_does_not_shadow_its_up_row() {
        let pair = reversible(1, "CREATE TABLE widget ()", "DROP TABLE widget");
        let reordered = [pair[1].clone(), pair[0].clone()];
        let rows = vec![applied(&pair[0])];

        let status = compare(reordered.iter(), &rows, None);

        assert_eq!(status.applied, 1);
        assert!(status.pending.is_empty());
        assert!(status.checksum_mismatch.is_empty());
        assert!(status.applied_not_embedded.is_empty());
        assert!(status.is_current());
    }

    #[test]
    fn a_reversible_pair_is_counted_once_alongside_simple_migrations() {
        let pair = reversible(2, "CREATE TABLE widget ()", "DROP TABLE widget");
        let embedded = [migration(1, "SELECT 1"), pair[0].clone(), pair[1].clone()];
        let rows = vec![applied(&embedded[0])];

        let status = compare(embedded.iter(), &rows, None);

        assert_eq!(status.applied, 1);
        assert_eq!(status.pending, vec![2]);
    }

    #[test]
    fn reported_versions_are_sorted() {
        let embedded = [
            migration(3, "SELECT 3"),
            migration(1, "SELECT 1"),
            migration(2, "SELECT 2"),
        ];

        let status = compare(embedded.iter(), &[], None);

        assert_eq!(status.pending, vec![1, 2, 3]);
    }
}
