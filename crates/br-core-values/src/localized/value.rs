//! The core [`Localized`] value: a primary locale plus per-locale entries,
//! validated at construction *and* on every deserialization.

use std::marker::PhantomData;

use serde::de::{Deserialize, Deserializer};
use serde::ser::{Serialize, SerializeStruct, Serializer};

use crate::error::ValueError;
use crate::localized::entry::LocalizedEntry;

/// A piece of text available in one or more locales, with one designated as
/// primary.
///
/// Generic over a type-level format marker `F` (plain / markdown / html — see
/// the `format` submodule) and the locale type `L`. The lib owns
/// no locale list: each product instantiates with its own closed `Locale` enum
/// (`En`/`Fr`/`Ja` here, `En`/`Zh` there). Prefer the
/// [`LocalizedString`](crate::LocalizedString) /
/// [`LocalizedMd`](crate::LocalizedMd) / [`LocalizedHtml`](crate::LocalizedHtml)
/// aliases over naming `F` directly.
///
/// # Invariants (enforced at construction **and** re-validated on deserialize)
/// 1. at least one entry,
/// 2. the `primary` locale has an entry,
/// 3. no duplicate locale across entries.
///
/// These hold by construction *and* on every wire read: deserialization runs the
/// same validating constructor and **fails closed** on a violation. This matters
/// because in an event-logged system serde is the main constructor path (every
/// hydration), not [`Localized::new`] — a payload like
/// `{"primary":"en","entries":[]}` must not deserialize into a value whose
/// [`primary`](Self::primary) accessor would later panic.
///
/// On the wire `F` does not appear — the serialized shape is exactly
/// `{ "primary": L, "entries": [ { "locale": L, "content": String } ] }`. The
/// locale's *string form* is `L`'s own serde representation, owned by the
/// product (the lib does not impose one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Localized<F, L> {
    primary: L,
    entries: Vec<LocalizedEntry<L>>,
    _format: PhantomData<F>,
}

impl<F, L> Localized<F, L> {
    /// Create a localized value with a single entry in `primary`. Always valid
    /// (one entry, primary present, no duplicate), so it is infallible.
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

    /// Rebuild a localized value from an explicit `primary` + `entries`,
    /// enforcing all three invariants. This is the validating constructor that
    /// deserialization routes through.
    ///
    /// # Errors
    /// - [`ValueError::LocalizedEmpty`] if `entries` is empty.
    /// - [`ValueError::LocalizedDuplicateLocale`] if a locale repeats.
    /// - [`ValueError::LocalizedPrimaryMissing`] if no entry matches `primary`.
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

    /// Add or replace the translation for `locale`. Preserves every invariant
    /// (set never empties, never duplicates, never drops the primary).
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

    /// Content for `locale`, if present.
    pub fn get(&self, locale: &L) -> Option<&str>
    where
        L: PartialEq,
    {
        self.entries
            .iter()
            .find(|e| &e.locale == locale)
            .map(|e| e.content.as_str())
    }

    /// The primary-locale content. Always present by invariant.
    pub fn primary(&self) -> &str
    where
        L: PartialEq,
    {
        self.get(&self.primary)
            .expect("invariant: primary locale always has an entry")
    }

    /// The primary locale itself.
    pub fn primary_locale(&self) -> &L {
        &self.primary
    }

    /// Iterate the entries in storage order. Exposed so callers (e.g. a GraphQL
    /// projection) can read every translation without round-tripping through
    /// `serde_json` to reach the private fields.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = &LocalizedEntry<L>> {
        self.entries.iter()
    }
}

impl<F, L> Serialize for Localized<F, L>
where
    L: Serialize,
{
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // `F` is intentionally not on the wire; serialize only the data shape.
        let mut s = serializer.serialize_struct("Localized", 2)?;
        s.serialize_field("primary", &self.primary)?;
        s.serialize_field("entries", &self.entries)?;
        s.end()
    }
}

/// Wire-shaped mirror used only as the deserialization landing zone. Deriving
/// `Deserialize` on it (not on `Localized`) keeps `F` out of the wire format and
/// forces every read through [`Localized::from_parts`] — closing the serde
/// backdoor (an unvalidated `Localized` can never exist).
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
