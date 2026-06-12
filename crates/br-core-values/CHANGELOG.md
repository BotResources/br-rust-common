# Changelog — br-core-values

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-06-12

**Added**
- Initial release. Universal, constructor-validated value objects (tier `core`,
  serde only — no I/O, no `async`, no `br-util-*` dependency). Every value
  validates at construction **and** re-validates on deserialization, failing
  closed: an illegal value can be neither built nor loaded from the wire.
  - `Localized<F, L>` — localized rich text, **generic over a type-level format
    marker `F`** (plain / markdown / html) **and the locale type `L`**. The lib
    owns no locale list; each product instantiates with its own closed `Locale`
    enum. Aliases: `LocalizedString<L>`, `LocalizedMd<L>`, `LocalizedHtml<L>`.
    Three invariants — at least one entry, the primary locale has an entry, no
    duplicate locale — enforced in the validating constructor (`from_parts`) and
    **re-run on every deserialization** (serde routes through `from_parts`, not a
    bypass): a payload like `{"primary":"en","entries":[]}` does not deserialize.
    Private fields read via `primary()` / `primary_locale()` / `get()` /
    `entries()` (the iterator avoids the GraphQL round-trip through
    `serde_json`). On the wire the format marker `F` is absent; the locale's
    string form is `L`'s own serde representation (the lib imposes none — a
    product gives its enum a single, recommended-lowercase wire form with
    `#[serde(alias)]` read-compat).
  - `LocalizedContent<L>` — a runtime tagged union over markdown and html
    (`{"format":"md"|"html","body":…}`). Invariants delegated to the inner
    `Localized`. `From<LocalizedMd<L>>` / `From<LocalizedHtml<L>>` give the
    documented legacy-compat lift for a bare (untagged) blob, which does not
    deserialize as `LocalizedContent` directly.
  - `Markdown` / `Html` / `PlainText` format markers (sealed `TextFormat`
    trait); `LocalizedEntry<L>` is the per-locale entry. `TextFormat::WIRE_TAG`
    is the format's wire discriminator — `md` / `html` are the live tags
    `LocalizedContent` uses; `PlainText`'s `plain` is **reserved** (PlainText
    never enters `LocalizedContent`), documented for consumer projection/lint.
  - **Locale casing norm (required):** a language locale (`L` in
    `Localized<F, L>`) is the ASCII-**lowercase** ISO 639-1 / BCP 47 language
    subtag (`en`/`fr`/`ja`) — a *different* ISO standard, with *different* casing,
    from the UPPERCASE `CountryCode` (ISO 3166-1) and `Currency` (ISO 4217) value
    objects. The legacy read-compat pattern (`#[serde(alias = "En")]`) keeps
    capitalized stored events parsing while new writes are lowercase.
  - `conformance::assert_lowercase_roundtrip` (feature `conformance`, off by
    default) — the reusable mechanism a product plugs its own `Locale` enum into,
    in its tests, to prove the enum obeys the lowercase norm (serializes to an
    ASCII-lowercase string and deserializes back). Serde-only (no `serde_json`);
    the lib owns the norm, not the locale list.
  - `LocalizedHtml` stores **raw HTML verbatim and does not sanitize** —
    sanitization is the rendering edge's responsibility (documented on the type).
  - `Currency` — ISO 4217 alphabetic code, validated against the 169 active
    codes (`CURRENCY_CODES`, exposed). Trims, uppercases; `RMB` and `ZZZ` are
    rejected. `as_str` / `AsRef<str>` / `Display`, no `Deref`.
  - `CountryCode` — ISO 3166-1 alpha-2 code, validated against all 249 codes
    (`COUNTRY_CODES`, exposed). `UK` and `ZZ` are rejected. Same accessors, no
    `Deref`.
  - `Money` — minor-unit `i64` amount + `Currency`. Negative amounts allowed
    (credits/refunds). **No arithmetic methods** (domain policy, not a
    value-object concern). Public fields — no invariant beyond the
    self-validating currency.
  - `ValueError` — the crate's own error type, one enum for every constructor.
    Per codes-not-language its `#[error("…")]` strings are **stable codes**
    (`malformed_code`, `unknown_currency`, `unknown_country`, `localized_empty`,
    `localized_primary_missing`, `localized_duplicate_locale`) with structured
    params, never UI prose. `#[non_exhaustive]`; (de)serializes (internally
    tagged on `code`) so a rejection reason can travel on the wire.
    **Forward-compat:** an `Unknown { code }` catch-all means an unrecognized
    `code` from a newer crate version degrades to `Unknown` on deserialization
    (preserving the raw code) instead of failing the whole enclosing envelope;
    known codes stay strongly typed. `Serialize`/`Deserialize` are hand-rolled
    for this (the derive cannot carry the `code` tag into a field), with the wire
    shape unchanged.

**Hardening vs the seed implementations (epic #38, issue #39)**
- The localized family is now **generic over the locale type** (was a fixed
  `Locale` enum baked into the seed).
- The **serde backdoor is closed**: every deserialization re-validates all three
  invariants (the seed deserialized `{"primary":…,"entries":[]}` fine and panicked
  later in `get_primary`).
- Constructors return the crate's own **`ValueError` (codes, not language)** — the
  money/country seed returned a human-readable `Err(String)`.
- ISO lookups use **`binary_search`** over the sorted code tables (was a linear
  `.contains()` scan); an `is_sorted` test per table guards the precondition.
- The ISO-list tests assert **content** (presence of recent codes — currency
  `SLE`/`ZWG`/`VED`, country `SS`/`BQ`/`CW`/`SX`; absence of retired/unofficial
  codes — currency `HRK`/`SLL`/`ZWL`, country `AN`/`CS`/`XK`) instead of
  re-asserting the list length — the tests now prove the list is *current*.
