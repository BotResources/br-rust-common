#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DirectoryError {
    #[error("extension key '{key}' shadows the reserved core field on {entity}")]
    ReservedExtensionKey { entity: &'static str, key: String },
}

pub(crate) fn reject_reserved_keys<'a, I>(
    entity: &'static str,
    reserved: I,
    extensions: &std::collections::BTreeMap<String, serde_json::Value>,
) -> Result<(), DirectoryError>
where
    I: IntoIterator<Item = &'a str>,
{
    for key in reserved {
        if extensions.contains_key(key) {
            return Err(DirectoryError::ReservedExtensionKey {
                entity,
                key: key.to_string(),
            });
        }
    }
    Ok(())
}
