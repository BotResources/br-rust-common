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
    trait); `LocalizedEntry<L>` is the per-locale entry.
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

**Hardening vs the seed implementations (epic #38, issue #39)**
- The localized family is now **generic over the locale type** (was a fixed
  `Locale` enum baked into the seed).
- The **serde backdoor is closed**: every deserialization re-validates all three
  invariants (the seed deserialized `{"primary":…,"entries":[]}` fine and panicked
  later in `get_primary`).
- Constructors return the crate's own **`ValueError` (codes, not language)** — the
  money/country seed returned a human-readable `Err(String)`.
