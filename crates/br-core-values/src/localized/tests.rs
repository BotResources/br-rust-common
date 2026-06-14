use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::error::ValueError;
use crate::localized::value::Localized;
use crate::localized::{
    Html, LocalizedContent, LocalizedEntry, LocalizedHtml, LocalizedMd, LocalizedString, Markdown,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Locale {
    En,
    #[serde(alias = "Ja")]
    Ja,
    Fr,
}

#[test]
fn new_creates_with_single_entry() {
    let ls = LocalizedString::new(Locale::En, "Hello".into());
    assert_eq!(ls.get(&Locale::En), Some("Hello"));
    assert_eq!(ls.primary(), "Hello");
    assert_eq!(ls.entries().len(), 1);
}

#[test]
fn primary_returns_primary_content_for_non_default_locale() {
    let ls = LocalizedString::new(Locale::Ja, "こんにちは".into());
    assert_eq!(ls.primary(), "こんにちは");
    assert_eq!(ls.primary_locale(), &Locale::Ja);
}

#[test]
fn get_returns_none_for_missing_locale() {
    let ls = LocalizedString::new(Locale::En, "Hello".into());
    assert_eq!(ls.get(&Locale::Fr), None);
}

#[test]
fn set_adds_new_translation() {
    let mut ls = LocalizedString::new(Locale::En, "Hello".into());
    ls.set(Locale::Fr, "Bonjour".into());
    assert_eq!(ls.get(&Locale::Fr), Some("Bonjour"));
    assert_eq!(ls.get(&Locale::En), Some("Hello"));
    assert_eq!(ls.entries().len(), 2);
}

#[test]
fn set_replaces_existing_translation_without_duplicating() {
    let mut ls = LocalizedString::new(Locale::En, "Hello".into());
    ls.set(Locale::En, "Hi".into());
    assert_eq!(ls.get(&Locale::En), Some("Hi"));
    assert_eq!(ls.entries().len(), 1);
}

#[test]
fn entries_iterator_exposes_all_translations() {
    let mut md = LocalizedMd::new(Locale::En, "# Title".into());
    md.set(Locale::Fr, "# Titre".into());
    let collected: Vec<(&Locale, &str)> = md
        .entries()
        .map(|e| (&e.locale, e.content.as_str()))
        .collect();
    assert!(collected.contains(&(&Locale::En, "# Title")));
    assert!(collected.contains(&(&Locale::Fr, "# Titre")));
}

#[test]
fn empty_content_string_is_allowed() {
    let ls = LocalizedString::new(Locale::En, String::new());
    assert_eq!(ls.primary(), "");
}

#[test]
fn new_trims_leading_and_trailing_whitespace() {
    let ls = LocalizedString::new(Locale::En, "  Hello  ".into());
    assert_eq!(ls.primary(), "Hello");
}

#[test]
fn new_preserves_interior_whitespace_and_blank_lines() {
    let markdown = "\n# Title\n\nFirst paragraph.\n\n    indented code\n\nLast.\n";
    let md = LocalizedMd::new(Locale::En, markdown.into());
    assert_eq!(
        md.primary(),
        "# Title\n\nFirst paragraph.\n\n    indented code\n\nLast."
    );
}

#[test]
fn trailing_newline_does_not_break_equality() {
    let clean = LocalizedString::new(Locale::En, "Hello".into());
    let padded = LocalizedString::new(Locale::En, "Hello\n".into());
    assert_eq!(clean, padded);
}

#[test]
fn whitespace_only_content_trims_to_empty_and_is_allowed() {
    let ls = LocalizedString::new(Locale::En, "   \n\t ".into());
    assert_eq!(ls.primary(), "");
}

#[test]
fn set_trims_content() {
    let mut ls = LocalizedString::new(Locale::En, "Hello".into());
    ls.set(Locale::Fr, "  Bonjour\n".into());
    assert_eq!(ls.get(&Locale::Fr), Some("Bonjour"));
    ls.set(Locale::En, "\tHi ".into());
    assert_eq!(ls.get(&Locale::En), Some("Hi"));
}

#[test]
fn from_parts_trims_every_entry() {
    let v = Localized::<Markdown, Locale>::from_parts(
        Locale::En,
        vec![
            LocalizedEntry {
                locale: Locale::En,
                content: "  # A  ".into(),
            },
            LocalizedEntry {
                locale: Locale::Fr,
                content: "\n# B\n".into(),
            },
        ],
    )
    .unwrap();
    assert_eq!(v.get(&Locale::En), Some("# A"));
    assert_eq!(v.get(&Locale::Fr), Some("# B"));
}

#[test]
fn deserialize_trims_content() {
    let wire = r#"{"primary":"en","entries":[{"locale":"en","content":"  padded  \n"}]}"#;
    let v: LocalizedMd<Locale> = serde_json::from_str(wire).unwrap();
    assert_eq!(v.primary(), "padded");
}

#[test]
fn deserialize_normalizes_trailing_newline_so_padded_wire_equals_clean_value() {
    let padded = r#"{"primary":"en","entries":[{"locale":"en","content":"Hello\n"}]}"#;
    let from_wire: LocalizedMd<Locale> = serde_json::from_str(padded).unwrap();
    assert_eq!(from_wire, LocalizedMd::new(Locale::En, "Hello".into()));
}

#[test]
fn from_parts_accepts_a_well_formed_value() {
    let v = Localized::<Markdown, Locale>::from_parts(
        Locale::En,
        vec![
            LocalizedEntry {
                locale: Locale::En,
                content: "# A".into(),
            },
            LocalizedEntry {
                locale: Locale::Fr,
                content: "# B".into(),
            },
        ],
    );
    assert!(v.is_ok());
}

#[test]
fn from_parts_rejects_empty_entries() {
    let v = Localized::<Markdown, Locale>::from_parts(Locale::En, vec![]);
    assert_eq!(v.unwrap_err(), ValueError::LocalizedEmpty);
}

#[test]
fn from_parts_rejects_primary_without_entry() {
    let v = Localized::<Markdown, Locale>::from_parts(
        Locale::Fr,
        vec![LocalizedEntry {
            locale: Locale::En,
            content: "# A".into(),
        }],
    );
    assert_eq!(v.unwrap_err(), ValueError::LocalizedPrimaryMissing);
}

#[test]
fn from_parts_rejects_duplicate_locale() {
    let v = Localized::<Markdown, Locale>::from_parts(
        Locale::En,
        vec![
            LocalizedEntry {
                locale: Locale::En,
                content: "# A".into(),
            },
            LocalizedEntry {
                locale: Locale::En,
                content: "# B".into(),
            },
        ],
    );
    assert_eq!(v.unwrap_err(), ValueError::LocalizedDuplicateLocale);
}

#[test]
fn serialized_shape_omits_format_marker_and_uses_locale_wire_form() {
    let md = LocalizedMd::new(Locale::En, "# Title".into());
    let value = serde_json::to_value(&md).unwrap();
    assert_eq!(
        value,
        json!({
            "primary": "en",
            "entries": [ { "locale": "en", "content": "# Title" } ]
        })
    );
}

#[test]
fn serde_roundtrip_preserves_all_entries() {
    let mut md = LocalizedMd::new(Locale::Ja, "## 概要".into());
    md.set(Locale::En, "## Summary".into());
    let json_str = serde_json::to_string(&md).unwrap();
    let back: LocalizedMd<Locale> = serde_json::from_str(&json_str).unwrap();
    assert_eq!(md, back);
}

#[test]
fn deserialize_rejects_empty_entries_closing_the_backdoor() {
    let wire = r#"{"primary":"en","entries":[]}"#;
    assert!(serde_json::from_str::<LocalizedMd<Locale>>(wire).is_err());
}

#[test]
fn deserialize_rejects_primary_without_entry() {
    let wire = r#"{"primary":"fr","entries":[{"locale":"en","content":"x"}]}"#;
    assert!(serde_json::from_str::<LocalizedMd<Locale>>(wire).is_err());
}

#[test]
fn deserialize_rejects_duplicate_locale() {
    let wire = r#"{"primary":"en","entries":[{"locale":"en","content":"a"},{"locale":"en","content":"b"}]}"#;
    assert!(serde_json::from_str::<LocalizedMd<Locale>>(wire).is_err());
}

#[test]
fn deserialize_accepts_legacy_locale_alias() {
    let wire = r#"{"primary":"Ja","entries":[{"locale":"Ja","content":"x"}]}"#;
    let v: LocalizedMd<Locale> = serde_json::from_str(wire).unwrap();
    assert_eq!(v.primary_locale(), &Locale::Ja);
}

#[test]
fn content_constructors_produce_the_right_variant() {
    let m = LocalizedContent::md(Locale::En, "# Title".into());
    assert!(matches!(m, LocalizedContent::Md(_)));
    assert_eq!(m.format(), "md");
    assert_eq!(m.primary(), "# Title");

    let h = LocalizedContent::html(Locale::En, "<h1>T</h1>".into());
    assert!(matches!(h, LocalizedContent::Html(_)));
    assert_eq!(h.format(), "html");
}

#[test]
fn content_md_wire_format_matches_spec() {
    let c = LocalizedContent::md(Locale::En, "# Title".into());
    assert_eq!(
        serde_json::to_value(&c).unwrap(),
        json!({
            "format": "md",
            "body": { "primary": "en", "entries": [ { "locale": "en", "content": "# Title" } ] }
        })
    );
}

#[test]
fn content_html_wire_format_matches_spec() {
    let c = LocalizedContent::html(Locale::Ja, "<h1>標題</h1>".into());
    assert_eq!(
        serde_json::to_value(&c).unwrap(),
        json!({
            "format": "html",
            "body": { "primary": "ja", "entries": [ { "locale": "ja", "content": "<h1>標題</h1>" } ] }
        })
    );
}

#[test]
fn content_serde_roundtrip() {
    let c = LocalizedContent::html(Locale::Fr, "<p>Bonjour</p>".into());
    let json_str = serde_json::to_string(&c).unwrap();
    let back: LocalizedContent<Locale> = serde_json::from_str(&json_str).unwrap();
    assert_eq!(c, back);
}

#[test]
fn legacy_bare_md_fails_tagged_union_and_lifts_via_from() {
    let legacy = json!({
        "primary": "en",
        "entries": [ { "locale": "en", "content": "# Legacy" } ]
    });

    assert!(
        serde_json::from_value::<LocalizedContent<Locale>>(legacy.clone()).is_err(),
        "bare LocalizedMd JSON must not deserialize as LocalizedContent",
    );

    let md: LocalizedMd<Locale> = serde_json::from_value(legacy).expect("legacy LocalizedMd");
    let lifted: LocalizedContent<Locale> = md.into();
    assert_eq!(lifted.format(), "md");
    assert_eq!(lifted.primary(), "# Legacy");
}

#[test]
fn content_from_html_produces_html_variant() {
    let h: LocalizedHtml<Locale> = Localized::<Html, Locale>::new(Locale::En, "<p>x</p>".into());
    let c: LocalizedContent<Locale> = h.into();
    assert!(matches!(c, LocalizedContent::Html(_)));
}

#[test]
fn md_and_html_are_distinct_types() {
    fn take_md(_: &LocalizedMd<Locale>) {}
    let md = LocalizedMd::new(Locale::En, "# x".into());
    take_md(&md);
}
