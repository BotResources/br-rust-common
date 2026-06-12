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

| Alias | Format | Use |
|---|---|---|
| `LocalizedString<L>` | plain | titles, names, labels |
| `LocalizedMd<L>` | markdown | descriptions, summaries, prompt outputs |
| `LocalizedHtml<L>` | raw html | interactive reports |
| `LocalizedContent<L>` | md **or** html (runtime tag) | a body that may be either |

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
- **`entries()` iterator.** Read every translation without round-tripping through
  `serde_json` to reach the private fields.
- **`LocalizedHtml` stores raw HTML verbatim and does not sanitize.**
  Sanitization/escaping is the **rendering edge's** responsibility — a value
  object cannot know the sink's escaping rules. Never render a `LocalizedHtml`
  into an HTML context without sanitizing at that edge.

### Wire format

`Localized<F, L>` serializes as `{ "primary": L, "entries": [ { "locale": L,
"content": String } ] }` — the format marker `F` is **not** on the wire. The
locale's string form is `L`'s **own** serde representation; the lib imposes none.

A product must give its `Locale` enum a single, stable wire form — the
recommendation is lowercase (`"en"`/`"fr"`, BCP-47 convention), with
`#[serde(alias = …)]` read-compat for any earlier form already in stored events.
Owning that here would mean owning the locale list, which the family
deliberately does not.

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

## Tier & dependencies

Tier `core`: depends only on `serde` (+ `thiserror` for the error type). No I/O,
no `async`, no `br-util-*`. Per-crate semver, distributed by git tag.
