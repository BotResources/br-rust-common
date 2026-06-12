use serde::{Deserialize, Serialize};

use crate::localized::value::Localized;
use crate::localized::{Html, LocalizedHtml, LocalizedMd, Markdown};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "format", content = "body", rename_all = "snake_case")]
#[serde(
    bound(serialize = "L: Serialize"),
    bound(deserialize = "L: Deserialize<'de> + PartialEq")
)]
pub enum LocalizedContent<L> {
    Md(LocalizedMd<L>),
    Html(LocalizedHtml<L>),
}

impl<L> LocalizedContent<L> {
    pub fn md(primary: L, content: String) -> Self
    where
        L: Clone,
    {
        Self::Md(Localized::<Markdown, L>::new(primary, content))
    }

    pub fn html(primary: L, content: String) -> Self
    where
        L: Clone,
    {
        Self::Html(Localized::<Html, L>::new(primary, content))
    }

    pub fn format(&self) -> &'static str {
        use crate::localized::format::TextFormat;
        match self {
            Self::Md(_) => Markdown::WIRE_TAG,
            Self::Html(_) => Html::WIRE_TAG,
        }
    }

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
