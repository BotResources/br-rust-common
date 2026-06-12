//! [`LocalizedContent`] — a tagged union over markdown and html localized text.

use serde::{Deserialize, Serialize};

use crate::localized::value::Localized;
use crate::localized::{Html, LocalizedHtml, LocalizedMd, Markdown};

/// Rich localized content that is **either** markdown **or** html, carrying the
/// format as a runtime tag for code that must accept both without committing to
/// one at compile time (report bodies, prompt outputs, …).
///
/// Generic over the locale `L`, like the rest of the family. Invariants are
/// fully delegated to the inner [`Localized`]: a `Md(md)` is valid iff `md` is.
///
/// # Wire format
/// - `{"format":"md","body":{"primary":…,"entries":[…]}}`
/// - `{"format":"html","body":{"primary":…,"entries":[…]}}`
///
/// The inner `body` is the inner [`LocalizedMd`] / [`LocalizedHtml`] JSON
/// verbatim; the wrapper only adds the `format` discriminator and nests the body.
///
/// A **bare** `LocalizedMd` blob (no `format` tag) — the shape produced before
/// `LocalizedContent` existed — does not deserialize directly (the tag is
/// required); recover it with `from_value::<LocalizedMd<L>>(payload).map(Into::into)`,
/// the documented legacy-compat path (see `From<LocalizedMd<L>>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "format", content = "body", rename_all = "snake_case")]
#[serde(
    bound(serialize = "L: Serialize"),
    bound(deserialize = "L: Deserialize<'de> + PartialEq")
)]
pub enum LocalizedContent<L> {
    /// Markdown body.
    Md(LocalizedMd<L>),
    /// Raw-HTML body (sanitized at the rendering edge, never here).
    Html(LocalizedHtml<L>),
}

impl<L> LocalizedContent<L> {
    /// Markdown convenience constructor.
    pub fn md(primary: L, content: String) -> Self
    where
        L: Clone,
    {
        Self::Md(Localized::<Markdown, L>::new(primary, content))
    }

    /// HTML convenience constructor.
    pub fn html(primary: L, content: String) -> Self
    where
        L: Clone,
    {
        Self::Html(Localized::<Html, L>::new(primary, content))
    }

    /// The wire discriminator (`"md"` | `"html"`) — handy for projection / lint
    /// code that needs the format as a string without matching the variant.
    pub fn format(&self) -> &'static str {
        use crate::localized::format::TextFormat;
        match self {
            Self::Md(_) => Markdown::WIRE_TAG,
            Self::Html(_) => Html::WIRE_TAG,
        }
    }

    /// Primary-locale content, delegating to the inner value.
    pub fn primary(&self) -> &str
    where
        L: PartialEq,
    {
        match self {
            Self::Md(m) => m.primary(),
            Self::Html(h) => h.primary(),
        }
    }
}

impl<L> From<LocalizedMd<L>> for LocalizedContent<L> {
    fn from(m: LocalizedMd<L>) -> Self {
        Self::Md(m)
    }
}

impl<L> From<LocalizedHtml<L>> for LocalizedContent<L> {
    fn from(h: LocalizedHtml<L>) -> Self {
        Self::Html(h)
    }
}
