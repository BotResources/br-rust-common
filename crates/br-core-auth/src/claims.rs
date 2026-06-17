use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PassportClaims(Map<String, Value>);

impl PassportClaims {
    pub fn new() -> Self {
        Self(Map::new())
    }

    pub fn from_map(map: Map<String, Value>) -> Self {
        Self(map)
    }

    pub fn from_value(value: Value) -> Result<Self, Value> {
        match value {
            Value::Object(map) => Ok(Self(map)),
            other => Err(other),
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    pub fn iter(&self) -> serde_json::map::Iter<'_> {
        self.0.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }
}

impl Serialize for PassportClaims {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PassportClaims {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        Self::from_value(value).map_err(|_| D::Error::custom("claims must be a JSON object"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn new_is_empty() {
        assert!(PassportClaims::new().is_empty());
        assert_eq!(PassportClaims::new().len(), 0);
    }

    #[test]
    fn from_value_accepts_object() {
        let claims = PassportClaims::from_value(json!({"email": "a@b.com"})).unwrap();
        assert_eq!(claims.get("email"), Some(&json!("a@b.com")));
    }

    #[test]
    fn from_value_rejects_null() {
        assert!(PassportClaims::from_value(Value::Null).is_err());
    }

    #[test]
    fn from_value_rejects_array() {
        assert!(PassportClaims::from_value(json!([1, 2, 3])).is_err());
    }

    #[test]
    fn from_value_rejects_scalar() {
        assert!(PassportClaims::from_value(json!(42)).is_err());
        assert!(PassportClaims::from_value(json!("x")).is_err());
    }

    #[test]
    fn deserialize_accepts_object() {
        let claims: PassportClaims = serde_json::from_str(r#"{"k":"v"}"#).unwrap();
        assert_eq!(claims.get("k"), Some(&json!("v")));
    }

    #[test]
    fn deserialize_rejects_null() {
        assert!(serde_json::from_str::<PassportClaims>("null").is_err());
    }

    #[test]
    fn deserialize_rejects_array() {
        assert!(serde_json::from_str::<PassportClaims>("[]").is_err());
    }

    #[test]
    fn deserialize_rejects_scalar() {
        assert!(serde_json::from_str::<PassportClaims>("42").is_err());
    }

    #[test]
    fn serializes_as_object() {
        let claims = PassportClaims::from_map({
            let mut m = Map::new();
            m.insert("a".into(), json!(1));
            m
        });
        let v = serde_json::to_value(&claims).unwrap();
        assert_eq!(v, json!({"a": 1}));
    }

    #[test]
    fn iter_yields_entries() {
        let claims = PassportClaims::from_value(json!({"a": 1, "b": 2})).unwrap();
        let keys: Vec<&String> = claims.iter().map(|(k, _)| k).collect();
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn roundtrip_preserves_entries() {
        let claims = PassportClaims::from_value(json!({"email": "a@b.com", "n": 3})).unwrap();
        let json = serde_json::to_string(&claims).unwrap();
        let back: PassportClaims = serde_json::from_str(&json).unwrap();
        assert_eq!(claims, back);
    }
}
