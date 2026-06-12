//! Outbox table-name validation — the single structural guard run before a table
//! name is ever interpolated into SQL.
//!
//! Postgres cannot bind an *identifier* as a parameter (only values), so the
//! table name is formatted into the query string. Rather than trust it by
//! comment, an unsafe name is made **unrepresentable**: only a plain unquoted
//! identifier (`^[a-z_][a-z0-9_]*$`) is accepted, and every entry point
//! (`stage_into`, `OutboxStore::new`) runs through [`validate_table`].

use crate::outbox::store::OutboxStoreError;

/// Validate an outbox table name against `^[a-z_][a-z0-9_]*$`. Anything else — a
/// quote, a space, a `;`, an uppercase letter, a schema qualifier — is rejected
/// as a typed [`OutboxStoreError::InvalidTable`] before it can reach the SQL
/// string. ASCII-only on purpose: a valid unquoted PG identifier here needs no
/// Unicode, and restricting the set keeps the guard auditable.
pub(crate) fn validate_table(table: &str) -> Result<(), OutboxStoreError> {
    let mut chars = table.chars();
    let valid = match chars.next() {
        Some(c) if c == '_' || c.is_ascii_lowercase() => {
            chars.all(|c| c == '_' || c.is_ascii_lowercase() || c.is_ascii_digit())
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(OutboxStoreError::InvalidTable {
            table: table.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::outbox::store::DEFAULT_TABLE;

    // GIVEN a plain identifier WHEN validated THEN it is accepted
    #[test]
    fn accepts_plain_identifiers() {
        for table in [
            DEFAULT_TABLE,
            "outbox",
            "_outbox",
            "svc_chat_outbox",
            "t0",
            "a_1_b_2",
        ] {
            assert!(validate_table(table).is_ok(), "{table} should be valid");
        }
    }

    // GIVEN a name that could break out of the identifier position WHEN validated
    // THEN it is a typed InvalidTable, never interpolated into SQL
    #[test]
    fn rejects_unsafe_or_malformed_names() {
        for table in [
            "",                        // empty
            "1outbox",                 // leading digit
            "Outbox",                  // uppercase (would need quoting)
            "out box",                 // space
            "out-box",                 // hyphen
            "outbox;DROP TABLE users", // injection attempt
            "outbox--",                // comment
            "\"outbox\"",              // already-quoted
            "schema.outbox",           // qualified name
        ] {
            let err = validate_table(table).unwrap_err();
            assert!(
                matches!(err, OutboxStoreError::InvalidTable { .. }),
                "{table:?} should be rejected as InvalidTable"
            );
        }
    }
}
