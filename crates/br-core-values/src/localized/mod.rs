mod codec;
mod content;
mod entry;
mod format;
mod value;

pub use codec::LocaleCodec;
pub use content::LocalizedContent;
pub use entry::LocalizedEntry;
pub use format::{Html, Markdown, PlainText, TextFormat};
pub use value::Localized;

pub type LocalizedString<L> = Localized<PlainText, L>;
pub type LocalizedMd<L> = Localized<Markdown, L>;
pub type LocalizedHtml<L> = Localized<Html, L>;

#[cfg(test)]
mod tests;
