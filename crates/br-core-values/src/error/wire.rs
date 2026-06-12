use std::fmt;

use serde::de::{self, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Deserializer, Serialize};

use crate::error::ValueError;

impl Serialize for ValueError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            ValueError::MalformedCode {
                value,
                expected_len,
            } => {
                let mut m = serializer.serialize_map(Some(3))?;
                m.serialize_entry("code", "malformed_code")?;
                m.serialize_entry("value", value)?;
                m.serialize_entry("expected_len", expected_len)?;
                m.end()
            }
            ValueError::UnknownCurrency { value } => {
                let mut m = serializer.serialize_map(Some(2))?;
                m.serialize_entry("code", "unknown_currency")?;
                m.serialize_entry("value", value)?;
                m.end()
            }
            ValueError::UnknownCountry { value } => {
                let mut m = serializer.serialize_map(Some(2))?;
                m.serialize_entry("code", "unknown_country")?;
                m.serialize_entry("value", value)?;
                m.end()
            }
            ValueError::LocalizedEmpty => single_code(serializer, "localized_empty"),
            ValueError::LocalizedPrimaryMissing => {
                single_code(serializer, "localized_primary_missing")
            }
            ValueError::LocalizedDuplicateLocale => {
                single_code(serializer, "localized_duplicate_locale")
            }
            ValueError::Unknown { code } => single_code(serializer, code),
        }
    }
}

fn single_code<S: Serializer>(serializer: S, code: &str) -> Result<S::Ok, S::Error> {
    let mut m = serializer.serialize_map(Some(1))?;
    m.serialize_entry("code", code)?;
    m.end()
}

impl<'de> Deserialize<'de> for ValueError {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(ValueErrorVisitor)
    }
}

struct ValueErrorVisitor;

impl<'de> Visitor<'de> for ValueErrorVisitor {
    type Value = ValueError;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a ValueError object tagged on `code`")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<ValueError, A::Error> {
        let mut code: Option<String> = None;
        let mut value: Option<String> = None;
        let mut expected_len: Option<usize> = None;

        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "code" => {
                    if code.is_some() {
                        return Err(de::Error::duplicate_field("code"));
                    }
                    code = Some(map.next_value()?);
                }
                "value" => {
                    if value.is_some() {
                        return Err(de::Error::duplicate_field("value"));
                    }
                    value = Some(map.next_value()?);
                }
                "expected_len" => {
                    if expected_len.is_some() {
                        return Err(de::Error::duplicate_field("expected_len"));
                    }
                    expected_len = Some(map.next_value()?);
                }
                _ => {
                    map.next_value::<de::IgnoredAny>()?;
                }
            }
        }

        let code = code.ok_or_else(|| de::Error::missing_field("code"))?;
        match code.as_str() {
            "malformed_code" => Ok(ValueError::MalformedCode {
                value: value.ok_or_else(|| de::Error::missing_field("value"))?,
                expected_len: expected_len
                    .ok_or_else(|| de::Error::missing_field("expected_len"))?,
            }),
            "unknown_currency" => Ok(ValueError::UnknownCurrency {
                value: value.ok_or_else(|| de::Error::missing_field("value"))?,
            }),
            "unknown_country" => Ok(ValueError::UnknownCountry {
                value: value.ok_or_else(|| de::Error::missing_field("value"))?,
            }),
            "localized_empty" => Ok(ValueError::LocalizedEmpty),
            "localized_primary_missing" => Ok(ValueError::LocalizedPrimaryMissing),
            "localized_duplicate_locale" => Ok(ValueError::LocalizedDuplicateLocale),
            _ => Ok(ValueError::Unknown { code }),
        }
    }
}
