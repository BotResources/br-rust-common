use std::collections::BTreeMap;

use serde_json::Value;

pub(crate) fn take_required_string(
    bag: &mut BTreeMap<String, Value>,
    key: &str,
) -> Result<String, String> {
    match bag.remove(key) {
        Some(Value::String(s)) => Ok(s),
        Some(other) => Err(format!("field '{key}' must be a string, got {other}")),
        None => Err(format!("missing required field '{key}'")),
    }
}

pub(crate) fn take_optional_string(
    bag: &mut BTreeMap<String, Value>,
    key: &str,
) -> Result<Option<String>, String> {
    match bag.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(other) => Err(format!("field '{key}' must be a string, got {other}")),
    }
}

pub(crate) fn take_required_uuid_vec(
    bag: &mut BTreeMap<String, Value>,
    key: &str,
) -> Result<Vec<uuid::Uuid>, String> {
    let value = bag
        .remove(key)
        .ok_or_else(|| format!("missing required field '{key}'"))?;
    serde_json::from_value(value).map_err(|e| format!("field '{key}' must be a uuid list: {e}"))
}
