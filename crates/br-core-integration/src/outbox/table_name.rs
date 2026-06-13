use crate::outbox::store::OutboxStoreError;

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

    #[test]
    fn rejects_unsafe_or_malformed_names() {
        for table in [
            "",
            "1outbox",
            "Outbox",
            "out box",
            "out-box",
            "outbox;DROP TABLE users",
            "outbox--",
            "\"outbox\"",
            "schema.outbox",
        ] {
            let err = validate_table(table).unwrap_err();
            assert!(
                matches!(err, OutboxStoreError::InvalidTable { .. }),
                "{table:?} should be rejected as InvalidTable"
            );
        }
    }
}
