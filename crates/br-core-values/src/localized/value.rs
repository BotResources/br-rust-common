use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::error::ValueError;
use crate::localized::entry::LocalizedEntry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localized<F, L> {
    primary: L,
    entries: Vec<LocalizedEntry<L>>,
    _format: PhantomData<F>,
}

impl<F, L> Localized<F, L> {
    pub fn new(primary: L, content: String) -> Self
    where
        L: Clone,
    {
        Self {
            entries: vec![LocalizedEntry {
                locale: primary.clone(),
                content,
            }],
            primary,
            _format: PhantomData,
        }
    }

    pub fn from_parts(primary: L, entries: Vec<LocalizedEntry<L>>) -> Result<Self, ValueError>
    where
        L: PartialEq,
    {
        if entries.is_empty() {
            return Err(ValueError::LocalizedEmpty);
        }
        for (i, e) in entries.iter().enumerate() {
            if entries[..i].iter().any(|prev| prev.locale == e.locale) {
                return Err(ValueError::LocalizedDuplicateLocale);
            }
        }
        if !entries.iter().any(|e| e.locale == primary) {
            return Err(ValueError::LocalizedPrimaryMissing);
        }
        Ok(Self {
            primary,
            entries,
            _format: PhantomData,
        })
    }

    pub fn set(&mut self, locale: L, content: String)
    where
        L: PartialEq,
    {
        if let Some(entry) = self.entries.iter_mut().find(|e| e.locale == locale) {
            entry.content = content;
        } else {
            self.entries.push(LocalizedEntry { locale, content });
        }
    }

    pub fn get(&self, locale: &L) -> Option<&str>
    where
        L: PartialEq,
    {
        self.entries
            .iter()
            .find(|e| &e.locale == locale)
            .map(|e| e.content.as_str())
    }

    pub fn primary(&self) -> &str
    where
        L: PartialEq,
    {
        self.get(&self.primary)
            .expect("invariant: primary locale always has an entry")
    }

    pub fn primary_locale(&self) -> &L {
        &self.primary
    }

    pub fn entries(&self) -> impl ExactSizeIterator<Item = &LocalizedEntry<L>> {
        self.entries.iter()
    }
}

impl<F, L> Serialize for Localized<F, L>
where
    L: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("Localized", 2)?;
        s.serialize_field("primary", &self.primary)?;
        s.serialize_field("entries", &self.entries)?;
        s.end()
    }
}

#[derive(serde::Deserialize)]
#[serde(bound = "L: serde::Deserialize<'de>")]
struct Unchecked<L> {
    primary: L,
    entries: Vec<LocalizedEntry<L>>,
}

impl<'de, F, L> Deserialize<'de> for Localized<F, L>
where
    L: Deserialize<'de> + PartialEq,
{
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Unchecked::<L>::deserialize(deserializer)?;
        Localized::from_parts(raw.primary, raw.entries).map_err(serde::de::Error::custom)
    }
}
