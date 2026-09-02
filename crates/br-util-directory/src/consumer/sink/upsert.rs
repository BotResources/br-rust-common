pub(crate) fn change_detecting_upsert(table: &str, key: &str, columns: &[&str]) -> String {
    assert!(
        !columns.is_empty(),
        "a change-detecting upsert needs at least one non-key column to compare"
    );
    let names = columns.join(", ");
    let placeholders = (2..=columns.len() + 1)
        .map(|position| format!("${position}"))
        .collect::<Vec<_>>()
        .join(", ");
    let assignments = columns
        .iter()
        .map(|column| format!("{column} = EXCLUDED.{column}"))
        .collect::<Vec<_>>()
        .join(", ");
    let current = joined(columns, "t");
    let incoming = joined(columns, "EXCLUDED");
    format!(
        "INSERT INTO {table} AS t ({key}, {names}) VALUES ($1, {placeholders}) \
         ON CONFLICT ({key}) DO UPDATE SET {assignments} \
         WHERE ({current}) IS DISTINCT FROM ({incoming})"
    )
}

fn joined(columns: &[&str], qualifier: &str) -> String {
    columns
        .iter()
        .map(|column| format!("{qualifier}.{column}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn between<'a>(sql: &'a str, open: &str, close: &str) -> &'a str {
        let start = sql.find(open).expect("the opening marker") + open.len();
        let rest = &sql[start..];
        let end = rest.find(close).expect("the closing marker");
        &rest[..end]
    }

    fn items(segment: &str) -> Vec<&str> {
        segment.split(", ").collect()
    }

    fn qualified(columns: &[&str], prefix: &str) -> Vec<String> {
        columns
            .iter()
            .map(|column| format!("{prefix}{column}"))
            .collect()
    }

    fn assert_every_column_reaches_every_site(columns: &[&str]) {
        let sql = change_detecting_upsert("known_users", "user_id", columns);

        assert!(sql.contains(" ON CONFLICT (user_id) DO UPDATE SET "));

        let inserted = items(between(&sql, "AS t (", ") VALUES ("));
        assert_eq!(inserted[0], "user_id");
        assert_eq!(inserted[1..], columns[..]);

        assert_eq!(
            items(between(&sql, ") VALUES (", ") ON CONFLICT ")).len(),
            columns.len() + 1
        );
        assert_eq!(
            items(between(&sql, "DO UPDATE SET ", " WHERE (")),
            columns
                .iter()
                .map(|column| format!("{column} = EXCLUDED.{column}"))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            items(between(&sql, " WHERE (", ") IS DISTINCT FROM (")),
            qualified(columns, "t.")
        );
        assert_eq!(
            items(between(&sql, ") IS DISTINCT FROM (", ")")),
            qualified(columns, "EXCLUDED.")
        );
    }

    #[test]
    fn every_column_reaches_the_four_sites_that_must_agree() {
        assert_every_column_reaches_every_site(&["email", "first_name", "extensions"]);
    }

    #[test]
    fn a_single_column_reaches_the_same_four_sites() {
        assert_every_column_reaches_every_site(&["name"]);
    }

    #[test]
    fn the_placeholder_count_follows_the_column_count() {
        let sql = change_detecting_upsert("t", "k", &["a", "b", "c"]);
        assert_eq!(
            items(between(&sql, ") VALUES (", ") ON CONFLICT ")),
            vec!["$1", "$2", "$3", "$4"]
        );
    }

    #[test]
    #[should_panic(expected = "at least one non-key column")]
    fn a_column_less_upsert_is_refused_rather_than_rendered() {
        change_detecting_upsert("known_users", "user_id", &[]);
    }
}
