use core::fmt;

use serde::de::IntoDeserializer;
use serde::de::value::StrDeserializer;
use serde::{Deserialize, Serialize, Serializer};

pub fn assert_lowercase_roundtrip<L>(locales: &[L])
where
    L: Serialize + for<'de> Deserialize<'de> + PartialEq + fmt::Debug,
{
    for locale in locales {
        let wire = serialize_to_string(locale)
            .unwrap_or_else(|e| panic!("{locale:?} must serialize to a string: {e}"));

        assert!(
            !wire.is_empty() && wire.chars().all(|c| c.is_ascii_lowercase()),
            "{locale:?} serializes to {wire:?}, which is not ASCII-lowercase \
             (language locales are lowercase BCP 47 / ISO 639-1 subtags)"
        );

        let de: StrDeserializer<'_, serde::de::value::Error> = wire.as_str().into_deserializer();
        let back = L::deserialize(de)
            .unwrap_or_else(|e| panic!("{locale:?} must deserialize back from {wire:?}: {e}"));
        assert!(
            back == *locale,
            "{locale:?} did not round-trip through its lowercase wire form {wire:?}"
        );
    }
}

fn serialize_to_string<T: Serialize>(value: &T) -> Result<String, StringOnlyError> {
    value.serialize(StringCapture)
}

#[derive(Debug)]
struct StringOnlyError(String);

impl fmt::Display for StringOnlyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for StringOnlyError {}

impl serde::ser::Error for StringOnlyError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        StringOnlyError(msg.to_string())
    }
}

struct StringCapture;

fn not_a_string(what: &str) -> StringOnlyError {
    StringOnlyError(format!("expected a single string, got {what}"))
}

impl Serializer for StringCapture {
    type Ok = String;
    type Error = StringOnlyError;
    type SerializeSeq = serde::ser::Impossible<String, StringOnlyError>;
    type SerializeTuple = serde::ser::Impossible<String, StringOnlyError>;
    type SerializeTupleStruct = serde::ser::Impossible<String, StringOnlyError>;
    type SerializeTupleVariant = serde::ser::Impossible<String, StringOnlyError>;
    type SerializeMap = serde::ser::Impossible<String, StringOnlyError>;
    type SerializeStruct = serde::ser::Impossible<String, StringOnlyError>;
    type SerializeStructVariant = serde::ser::Impossible<String, StringOnlyError>;

    fn serialize_str(self, v: &str) -> Result<String, StringOnlyError> {
        Ok(v.to_owned())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _index: u32,
        variant: &'static str,
    ) -> Result<String, StringOnlyError> {
        Ok(variant.to_owned())
    }

    fn serialize_bool(self, _: bool) -> Result<String, StringOnlyError> {
        Err(not_a_string("a bool"))
    }
    fn serialize_i8(self, _: i8) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_i16(self, _: i16) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_i32(self, _: i32) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_i64(self, _: i64) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_u8(self, _: u8) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_u16(self, _: u16) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_u32(self, _: u32) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_u64(self, _: u64) -> Result<String, StringOnlyError> {
        Err(not_a_string("an integer"))
    }
    fn serialize_f32(self, _: f32) -> Result<String, StringOnlyError> {
        Err(not_a_string("a float"))
    }
    fn serialize_f64(self, _: f64) -> Result<String, StringOnlyError> {
        Err(not_a_string("a float"))
    }
    fn serialize_char(self, _: char) -> Result<String, StringOnlyError> {
        Err(not_a_string("a char"))
    }
    fn serialize_bytes(self, _: &[u8]) -> Result<String, StringOnlyError> {
        Err(not_a_string("bytes"))
    }
    fn serialize_none(self) -> Result<String, StringOnlyError> {
        Err(not_a_string("none"))
    }
    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<String, StringOnlyError> {
        Err(not_a_string("some"))
    }
    fn serialize_unit(self) -> Result<String, StringOnlyError> {
        Err(not_a_string("unit"))
    }
    fn serialize_unit_struct(self, _: &'static str) -> Result<String, StringOnlyError> {
        Err(not_a_string("a unit struct"))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        v: &T,
    ) -> Result<String, StringOnlyError> {
        v.serialize(self)
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: &T,
    ) -> Result<String, StringOnlyError> {
        Err(not_a_string("a newtype variant"))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, StringOnlyError> {
        Err(not_a_string("a sequence"))
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, StringOnlyError> {
        Err(not_a_string("a tuple"))
    }
    fn serialize_tuple_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleStruct, StringOnlyError> {
        Err(not_a_string("a tuple struct"))
    }
    fn serialize_tuple_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeTupleVariant, StringOnlyError> {
        Err(not_a_string("a tuple variant"))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, StringOnlyError> {
        Err(not_a_string("a map"))
    }
    fn serialize_struct(
        self,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStruct, StringOnlyError> {
        Err(not_a_string("a struct"))
    }
    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        _: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant, StringOnlyError> {
        Err(not_a_string("a struct variant"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum Locale {
        En,
        Fr,
        Ja,
    }

    #[test]
    fn lowercase_locale_enum_passes() {
        assert_lowercase_roundtrip(&[Locale::En, Locale::Fr, Locale::Ja]);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    enum BadLocale {
        En,
    }

    #[test]
    #[should_panic(expected = "not ASCII-lowercase")]
    fn pascalcase_locale_is_rejected() {
        assert_lowercase_roundtrip(&[BadLocale::En]);
    }

    #[test]
    fn char_is_rejected_by_the_serializer() {
        let err = serialize_to_string(&'a').unwrap_err();
        assert_eq!(err.to_string(), "expected a single string, got a char");
    }
}
