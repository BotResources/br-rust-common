pub trait TextFormat: private::Sealed {
    const WIRE_TAG: &'static str;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlainText {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Markdown {}

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
