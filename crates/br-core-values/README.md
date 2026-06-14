# br-core-values

Universal, constructor-validated **value objects** shared across BotResources
services. Tier `core` — serde only, **no I/O, no `async`, and no dependency on
any `br-util-*` crate**. Two families: the `Localized<F, L>` rich-text family
(generic over the locale type) and the ISO value objects (`Money`, `Currency`,
`CountryCode`).

**Purpose.** Make illegal states unrepresentable for the values every BR service
re-implements: localized rich text and ISO-backed money/currency/country. Each
value validates at construction *and* on every deserialization, so an invalid
value can be neither built nor loaded from the wire.

**When to use.** A service needs localized text (titles, descriptions, report
bodies) or a monetary/currency/country value. Instantiate the localized family
with the service's own closed `Locale` enum.

**When not to use.** A domain-specific value with project-specific invariants —
that belongs in the project's own shared kernel or domain, not here. This crate
holds only the *universal* values.

## The `Localized<F, L>` family

Text available in one or more locales, with one designated **primary**. Generic
over a type-level **format** marker `F` and the **locale** type `L`.

| Alias | Format | Wire tag | Use |
|---|---|---|---|
| `LocalizedString<L>` | plain | `plain` (reserved) | titles, names, labels |
| `LocalizedMd<L>` | markdown | `md` | descriptions, summaries, prompt outputs |
| `LocalizedHtml<L>` | raw html | `html` | interactive reports |
| `LocalizedContent<L>` | md **or** html (runtime tag) | `md` / `html` | a body that may be either |

The `md` / `html` wire tags are the live discriminators `LocalizedContent` reads
and writes. `PlainText::WIRE_TAG` (`plain`) is **reserved**: `PlainText` never
enters `LocalizedContent` (that union is md-or-html only), so the tag is not used
on the wire by this crate — it is provided for consumer projection/lint code that
wants a uniform format string per marker.

- **Generic over the locale; the lib owns no locale list.** Each product
  supplies its own closed `Locale` enum (`En`/`Fr`/`Ja` here, `En`/`Zh` there)
  and instantiates the family with it. A fixed locale list in the lib would be
  the wrong owner.
- **Type-level md/html distinction.** The format is a zero-sized marker carried
  in the type, so `LocalizedMd<L>` and `LocalizedHtml<L>` are *distinct types* a
  function cannot mix up — the compiler enforces it, not a runtime tag.
- **Three invariants, enforced at construction *and* on deserialize:** at least
  one entry, the primary locale has an entry, no duplicate locale. The
  deserialization path runs the same validating constructor (`from_parts`) and
  **fails closed** — a payload like `{"primary":"en","entries":[]}` does *not*
  deserialize. This matters because in an event-logged system serde is the main
  constructor path (every hydration), not `new()`.
- **Content is trimmed at construction; interior whitespace is preserved.**
  Every construction path (`new`, `from_parts`, `set`, and therefore both the
  `Deserialize` path and the `br-util-graphql` input bridge, which route through
  `from_parts`) normalizes each entry's content with `str::trim()` — leading and
  trailing whitespace only. Interior whitespace is **never** altered: Markdown
  indentation, blank lines between paragraphs and code-block whitespace are
  semantic and survive verbatim. The guarantee is that two contents differing
  only by surrounding whitespace (e.g. a trailing `\n`) are `Eq` and serialize
  identically. Whitespace-only content trims to the empty string, which remains
  allowed — the value object normalizes, it does not enforce required-ness (a
  domain seam).
- **`entries()` iterator.** Read every translation without round-tripping through
  `serde_json` to reach the private fields.
- **`LocalizedHtml` stores raw HTML verbatim and does not sanitize.**
  Sanitization/escaping is the **rendering edge's** responsibility — a value
  object cannot know the sink's escaping rules. Never render a `LocalizedHtml`
  into an HTML context without sanitizing at that edge.

### Wire format

`Localized<F, L>` serializes as `{ "primary": L, "entries": [ { "locale": L,
"content": String } ] }` — the format marker `F` is **not** on the wire. The
locale's string form is `L`'s **own** serde representation; the lib imposes the
*list* on no one, but it does impose the **casing norm** below.

### Locale & code casing — different ISO standards, different casing (required)

These are **distinct ISO standards** with **distinct casing**; do not conflate a
lowercase language locale with the uppercase code value objects.

| Concept | Standard | Casing | Wire form | Where |
|---|---|---|---|---|
| Language locale (`L` in `Localized<F, L>`) | ISO 639-1 / BCP 47 language subtag | **lowercase** | `en`, `fr`, `ja` | product's `Locale` enum |
| `CountryCode` | ISO 3166-1 alpha-2 | **UPPERCASE** | `FR`, `JP` | this crate |
| `Currency` | ISO 4217 | **UPPERCASE** | `EUR`, `JPY` | this crate |
| Full locale tag (if ever combined) | BCP 47 | language **lowercase** + region **UPPERCASE**, hyphen | `en-US` | — (see note) |

- **Language locales are lowercase.** A product **must** give its `Locale` enum a
  single, stable, ASCII-lowercase wire form (`"en"`/`"fr"`/`"ja"`), with
  `#[serde(alias = …)]` read-compat for any earlier (e.g. capitalized) form
  already persisted in stored events — old writes still parse, new writes are
  lowercase. Owning the list here would mean owning the locale set, which the
  family deliberately does not; owning the *norm* it does.
- **`Localized` is language-only today** — it carries the language subtag, not a
  full BCP 47 `en-US` region tag. The combined-tag casing is noted for
  completeness only; if a product ever needs region, that is its own value object.

#### Proving conformance (feature `conformance`)

The lib ships the *mechanism* to prove a product's `Locale` enum obeys the
lowercase norm, without owning the *list*. Enable the `conformance` feature in
your dev build and plug your enum into `assert_lowercase_roundtrip` from your own
tests — it asserts each locale serializes to an ASCII-lowercase string **and**
deserializes back from that lowercase form (round-trip):

```toml
[dev-dependencies]
br-core-values = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-values", tag = "v0.11.0", features = ["conformance"] }
```

```rust,ignore
#[test]
fn locale_is_lowercase_conformant() {
    br_core_values::conformance::assert_lowercase_roundtrip(&[
        Locale::En, Locale::Fr, Locale::Ja,
    ]);
}
```

The feature is **off by default** so the helper never bloats the prod surface.

`LocalizedContent<L>` wraps the inner body and adds a `format` discriminator:

```json
{"format":"md","body":{"primary":"en","entries":[{"locale":"en","content":"# Title"}]}}
```

A **bare** `LocalizedMd` blob (no `format` tag) — the shape produced before
`LocalizedContent` existed — does **not** deserialize directly as
`LocalizedContent`; recover it with
`serde_json::from_value::<LocalizedMd<L>>(payload).map(LocalizedContent::from)`,
the documented legacy-compat lift (`From<LocalizedMd<L>>` /
`From<LocalizedHtml<L>>`).

## ISO value objects

- **`Currency`** — ISO 4217 alphabetic code, validated against the 169 active
  codes (`CURRENCY_CODES`). Trims and uppercases; `RMB` is rejected (`CNY` is
  correct), `ZZZ` is rejected. Precious-metal and numeric codes are out of scope.
- **`CountryCode`** — ISO 3166-1 alpha-2 code, validated against all 249 codes
  (`COUNTRY_CODES`). `UK` is rejected (`GB` is correct), `ZZ` is rejected.
- **`Money`** — a minor-unit `i64` amount plus a `Currency`. Negative amounts
  represent credits/refunds. **No arithmetic methods** — monetary arithmetic
  (rounding, conversion, allocation) is domain policy, not a value-object
  concern. `Money`'s fields are public because it has no invariant of its own
  beyond the currency (which is self-validating even through derived deserialize).

All three are self-validating newtypes (`new` returns `Result<Self,
ValueError>`); deserialization re-runs the constructor and fails closed.
`Currency`/`CountryCode` expose the code via `as_str()` / `AsRef<str>` /
`Display` — **no `Deref`**.

## Errors — codes, not language

Every constructor returns this crate's own `ValueError`. Per the
codes-not-language rule its `Display` strings are **stable codes**
(`malformed_code`, `unknown_currency`, `unknown_country`, `localized_empty`,
`localized_primary_missing`, `localized_duplicate_locale`) carrying structured
params — **never UI prose**. The human text and its i18n live at the edge.
`ValueError` is `#[non_exhaustive]` (match with a wildcard) and (de)serializes
(internally tagged on `code`) so a rejection reason can travel on the wire.

**Forward-compat on the wire.** `ValueError` travels nested in other envelopes
(domain errors, affordance reasons). A newer producer crate may emit a `code`
this (older) crate does not know yet. Rather than fail the deserialization of the
**whole** enclosing envelope, an unrecognized `code` degrades to
`ValueError::Unknown { code }` carrying the raw `code` string — the envelope
still parses, and the original code is preserved verbatim for logging /
pass-through. Every code this version knows stays strongly typed; only a genuinely
unknown future code degrades. (`Unknown` is produced **only** on deserialization,
never by a rejecting constructor here.)

## Tier & dependencies

Tier `core`: depends only on `serde` (+ `thiserror` for the error type). No I/O,
no `async`, no `br-util-*`. Unified workspace versioning, distributed by git tag.

## Install

```toml
[dependencies]
br-core-values = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-values", tag = "v0.11.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
