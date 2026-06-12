//! A single localized entry: one locale paired with its content string.

use serde::{Deserialize, Serialize};

/// One content entry for a specific locale, inside a
/// [`Localized`](crate::Localized) value.
///
/// Generic over the locale type `L` — the lib owns no locale list; each product
/// supplies its own closed `Locale` enum (see the crate docs). `content` is a
/// plain `String`; its *format* (plain / markdown / html) is carried by the
/// enclosing `Localized`'s type-level format marker, not here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalizedEntry<L> {
    /// The locale this entry is written in.
    pub locale: L,
    /// The content, in the format declared by the enclosing `Localized`.
    pub content: String,
}
