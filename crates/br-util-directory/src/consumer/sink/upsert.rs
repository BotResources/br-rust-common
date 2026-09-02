pub(crate) fn change_detecting_upsert(table: &str, key: &str, columns: &[&str]) -> String {
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

    #[test]
    fn one_column_renders_a_guarded_upsert() {
        assert_eq!(
            change_detecting_upsert("known_groups", "group_id", &["name"]),
            "INSERT INTO known_groups AS t (group_id, name) VALUES ($1, $2) \
             ON CONFLICT (group_id) DO UPDATE SET name = EXCLUDED.name \
             WHERE (t.name) IS DISTINCT FROM (EXCLUDED.name)"
        );
    }

    #[test]
    fn every_column_reaches_the_four_sites_that_must_agree() {
        assert_eq!(
            change_detecting_upsert("known_users", "user_id", &["email", "extensions"]),
            "INSERT INTO known_users AS t (user_id, email, extensions) VALUES ($1, $2, $3) \
             ON CONFLICT (user_id) DO UPDATE SET email = EXCLUDED.email, \
             extensions = EXCLUDED.extensions \
             WHERE (t.email, t.extensions) IS DISTINCT FROM (EXCLUDED.email, EXCLUDED.extensions)"
        );
    }

    #[test]
    fn the_placeholder_count_follows_the_column_count() {
        let sql = change_detecting_upsert("t", "k", &["a", "b", "c"]);
        assert!(sql.contains("VALUES ($1, $2, $3, $4)"));
    }
}
