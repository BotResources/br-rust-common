//! Text-format markers for the [`Localized`](crate::Localized) family.
//!
//! A format is carried **at the type level** by a zero-sized marker, so a
//! `LocalizedMd` and a `LocalizedHtml` are distinct types that cannot be passed
//! interchangeably — the md/html distinction is enforced by the compiler, not by
//! a runtime tag. The marker is never serialized (it is a `PhantomData` field on
//! the localized value); its only on-the-wire role is the discriminator string
//! that [`LocalizedContent`](crate::LocalizedContent) writes for the md/html
//! tagged union.

/// Sealed trait implemented by the format markers ([`PlainText`], [`Markdown`],
/// [`Html`]). Sealed so the format set is closed to this crate — a consumer
/// chooses among the provided formats but cannot invent a fourth.
pub trait TextFormat: private::Sealed {
    /// The stable wire discriminator for this format (`"plain"`, `"md"`,
    /// `"html"`), used by the [`LocalizedContent`](crate::LocalizedContent)
    /// tagged union and by projection/lint code that needs it as a string.
    const WIRE_TAG: &'static str;
}

/// Plain text — short human-readable fields: titles, names, labels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlainText {}

/// Markdown — descriptions, summaries, prompt outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Markdown {}

/// Raw HTML — interactive reports. **Stores raw HTML verbatim; it does not
/// sanitize.** Sanitization/escaping is the rendering edge's responsibility (a
/// value object cannot know the sink's escaping rules), so never render a
/// `LocalizedHtml` into an HTML context without sanitizing at that edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Html {}

impl TextFormat for PlainText {
    const WIRE_TAG: &'static str = "plain";
}
impl TextFormat for Markdown {
    const WIRE_TAG: &'static str = "md";
}
impl TextFormat for Html {
    const WIRE_TAG: &'static str = "html";
}

mod private {
    pub trait Sealed {}
    impl Sealed for super::PlainText {}
    impl Sealed for super::Markdown {}
    impl Sealed for super::Html {}
}
