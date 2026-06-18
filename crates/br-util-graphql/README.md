# br-util-graphql

The **GraphQL/REST edge kit** every BotResources service imports. Tier `util` —
technical wrappers (types / functions / traits), **no domain**: no aggregate, no
event, no projector, no `CommandResult`. It *supports* the collaborative-pure
doctrine (the affordance shape, the ack-only mutation result, the push payload,
the error contract every frontend binds to); it embodies none of it.

**Purpose.** Stop every service re-implementing — and *diverging* on — the same
edge plumbing: the error-code set, the affordance type, the mutation ack, cursor
pagination, the subscription payload, and the fallible value-object wrappers.
Before this crate, ~six diverging copies of the error mapping lived across the
estate (each `svc-*/src/error.rs`); this crate makes the set the single source of
truth.

**When to use.** Any service exposing an `async-graphql` subgraph and/or a REST
edge. **When not to use.** A crate that wires no edge — it can depend on this one
transitively without enabling the feature, paying nothing.

Everything sits behind the **`graphql`** feature (off by default). With no
features the crate compiles to nothing.

## The error-code contract (cross-service — authoritative)

`ErrorCode` is a **published contract every BR frontend binds to**. A failure
surfaces one of these strings in the GraphQL `extensions.code` field and as the
`code` of the REST error body; the client maps the code → localized copy. The
strings are **stable keys, never UI prose** (codes-not-language) — renaming one
is a breaking change across the whole estate, not a local edit.

| `ErrorCode` | Wire string | REST HTTP status | Meaning |
|---|---|---|---|
| `Unauthenticated` | `UNAUTHENTICATED` | 401 | no / invalid authenticated principal |
| `Forbidden` | `FORBIDDEN` | 403 | known principal, not permitted |
| `NotFound` | `NOT_FOUND` | 404 | resource absent / not visible |
| `Conflict` | `CONFLICT` | 409 | conflicts with current state (uniqueness, already-done) |
| `BadUserInput` | `BAD_USER_INPUT` | 400 | malformed / failed validation |
| `InvalidState` | `INVALID_STATE` | 422 | aggregate state forbids the transition |
| `Internal` | `INTERNAL` | 500 | server fault — detail logged, **never** returned |

`EdgeError` is the application-layer error a service builds from its own domain
error: a class `ErrorCode` + an optional precise `reason_code` (the broken domain
rule, also a stable code) + structured `params` + a never-returned internal
`detail`. It renders two ways:

- **GraphQL** — `EdgeError::into_gql()` → an `async_graphql::Error` whose
  `message` is the code and whose `extensions` carry `code`, `reason`?, and a
  nested `params`? object. `impl From<EdgeError> for async_graphql::Error` is
  provided, so a resolver returns `Result<_, EdgeError>` and `?` does the rest.
- **REST** — `impl IntoResponse for EdgeError` → the same-shaped JSON body
  `{ "error": { "code", "reason"?, "params"? } }` with the mapped HTTP status.

Both edges carry the identical `{ code, reason?, params? }` shape (`params` a
nested object on each), so a client reading either through one mapper sees the
same structure.

An `Internal` fault logs its `detail` (via `tracing`) at render time and returns
only the code — SQL / serialization / panic text never reaches a client.

A consuming service writes one `impl From<MyDomainError> for EdgeError` and every
resolver / handler / endpoint becomes uniform.

## The other edge types

- **`Affordance { action, allowed, reason_code, params? }`** — the dumb-frontend
  contract. The domain computes it and projects it (in the read snapshot *and*
  re-emitted on every state-changing event); the client renders it, never
  re-deriving "can I click this?". Build with `Affordance::allow(action)` or
  `Affordance::block(action, reason_code)` — a **blocked affordance must carry a
  code** (no silent denial), and `reason_code` is a key, never a sentence.
  `params` is an **optional** structured map keyed to `reason_code` (e.g.
  `{ "min_members": "1" }` on `system_group_protected`) — attach it with the
  chainable `.with_param(key, value)` / `.with_params(...)`. It is the **same Rust
  type** (`BTreeMap<String, String>`) with the **same builder ergonomics** as
  `EdgeError::params`, but **not the same wire shape**: `EdgeError` renders its
  params as a nested object inside the GraphQL `extensions`, whereas
  `Affordance.params` is exposed on the wire as a **nullable `JSON` scalar field**
  (`params: JSON`). It carries **codes, never UI prose** (the client localizes
  using `reason_code` + `params`); `allow` / `block` leave it `null`, so existing
  producers and their wire output are unchanged.
- **`MutationResult`** — the success ack (`{ success: true }`). A mutation
  returns this or an `EdgeError`, **never a DTO** (R1, collaborative-pure): new
  state reaches clients via the event stream, so two clients can't diverge. The
  type has no field to smuggle state back. (The one estate-wide exception — a
  one-shot secret returned once — is a *different*, deliberately-named return
  type a service defines for that mutation; never bolted onto this ack.)
- **`Connection<T>` / `Edge<T>` / `PageInfo`** — reusable cursor pagination in
  the Relay-flavoured shape (`edges { node cursor }` + `pageInfo`). The cursor is
  an **opaque String** the server mints and the client echoes via `after`; this
  crate does not prescribe its encoding. `Connection::forward(edges,
  has_next_page)` builds a forward page and derives the boundary cursors.
- **`SubscriptionPayload<N, E, T>`** — the collaborative-pure push: the **event**
  that happened (the near-unfiltered domain-event union), the **fresh entity** it
  produced, and the **recalculated affordances** for *this* subscriber — so a
  client folds it into state without a refetch. Generic over a `PayloadName` `N`
  and the event union `E` and entity `T`, both `async_graphql::OutputType`. `N`
  supplies the wire type name via `PayloadName::NAME`: that string **must be a
  valid GraphQL identifier and unique per `(event-union, entity)` pairing** — a
  malformed or duplicated name fails schema composition at boot (fail-loud), so
  keeping each `PayloadName::NAME` distinct is the caller's contract.

## Fallible value-object wrappers (`values`)

`TryFrom` / seam conversions over [`br-core-values`](../br-core-values/README.md)
that **fail with a typed code, never silently coerce** — the deliberate inverse
of the Hanshow seed (which coerced unknown locales to the default, accepted
missing primary content, and truncated `Money` i64→i32):

| Wrapper | Conversion | Fails with |
|---|---|---|
| `GqlMoney` (output) | `From<&Money>` | **infallible** — the full `i64` minor-unit amount is carried by the `MoneyAmount` scalar (a decimal string), no ceiling, **never truncates** |
| `GqlMoneyInput` (input) | `TryFrom<GqlMoneyInput> for Money` | `MONEY_OUT_OF_RANGE` if the inbound `MoneyAmount` string is non-numeric or overflows `i64`; the currency's own code (e.g. `unknown_currency`) if the ISO code is unknown — **never coerced, never truncated** |
| `GqlLocalizedInput` (input) | `into_localized::<F, L>()` | `LOCALE_UNKNOWN` (any unknown locale) / `PRIMARY_CONTENT_MISSING` (no entry for the primary) / the value object's own code (empty, duplicate) |
| `GqlLocalized` (output) | `from_localized::<F, L>(&Localized<F, L>)` | **infallible** — projects the domain value to a `SimpleObject` carrying `primaryLocale` (the canonical locale's wire code) and `entries` (every locale, the primary included) |
| `GqlLocale` (trait — re-export of `br_core_values::LocaleCodec`) | `parse_wire(&str)` / `as_wire(&self)` | `locale_unknown` on parse (mapped to `LOCALE_UNKNOWN` at the edge) — the product-supplied seam that owns **both directions** of the wire↔locale mapping: `from_wire` (string → locale, fallible) and `as_wire` (locale → string, total) |

`GqlValueError` carries the rejection (`LOCALE_UNKNOWN`, `MONEY_OUT_OF_RANGE`,
`PRIMARY_CONTENT_MISSING`, or a wrapped value-object code) and converts to an
`EdgeError` with `code = BAD_USER_INPUT` + the precise `reason` + the offending
value as a param.

**`Money` carries the full `i64` on the wire as a decimal string — no ceiling.**
`GqlMoney`/`GqlMoneyInput` carry the minor-unit amount through a dedicated
`MoneyAmount` scalar that serializes/parses the whole `i64` range as a **decimal
string** (e.g. `"123456789012"`), not the built-in `Int`. Two reasons it is a
string, not a number:

- GraphQL's built-in `Int` is **32-bit by spec**, so it caps a money amount at
  `i32::MAX` minor units (≈ 21.5 M for a 2-decimal currency) — far too small for
  B2B. The fix is to stop using `Int` for money, not to keep the cap.
- A *numeric* scalar over `i64` is **not precision-safe either**: JSON numbers are
  IEEE-754 doubles and lose integer precision above 2⁵³, so a large `i64` would
  silently corrupt in any JS/JSON client. A **decimal string** is the standard,
  JS-safe large-integer-money representation (the GitHub / Stripe convention).

Output (`From<&Money>`) is therefore **infallible**: every `i64` round-trips
exactly, never truncated, no range bound below `i64`. The `MONEY_OUT_OF_RANGE`
code now fires **only on input**, when an inbound `MoneyAmount` string is
non-numeric or overflows `i64` — the parse/overflow boundary, not an `Int` ceiling
(it carries the offending raw string as its `amount` param). An input of the wrong
GraphQL *type* (e.g. a JSON number instead of a string) is rejected by the scalar
as a type error *before* it reaches `MONEY_OUT_OF_RANGE` — the wire contract is a
decimal string, fail-closed.

Where this code surfaces depends on the layer. When `MoneyAmount` is deserialized
by async-graphql itself (the nominal case — a `MoneyAmount!` argument),
`MONEY_OUT_OF_RANGE` arrives in the GraphQL **`error.message`** (`Failed to parse
"MoneyAmount": MONEY_OUT_OF_RANGE`), **not** in `extensions.code`/`params` like the
codes that flow through `EdgeError` — an inherent limit of the GraphQL scalar-parse
layer (it has no structured-extension channel). Key on the stable substring
`MONEY_OUT_OF_RANGE`, never on the async-graphql prefix (which is version-bound).
The structured `extensions` surface stays available for a service that takes the
string itself via `GqlMoneyInput`'s `TryFrom` → `EdgeError`.

**Reason-code casing on the `reason` channel.** Codes minted by this crate are
`UPPER_SNAKE` (`LOCALE_UNKNOWN`, `MONEY_OUT_OF_RANGE`, `PRIMARY_CONTENT_MISSING`);
a value-object rejection passed through (`unknown_currency`, `localized_empty`, …)
keeps `br-core-values`'s own `lower_snake` casing **verbatim**, by design — the
codes are not re-cased at the boundary because that would break their stability as
keys. A client reading the `reason` field must therefore key on the exact string,
not assume one casing convention.

The locale seam (`GqlLocale`) exists because **the lib owns no locale list**
(neither does `br-core-values`): each product implements `from_wire` and `as_wire`
on its own `Locale` enum — the trait owns both directions of the wire↔locale
mapping, so the input wrappers refuse anything `from_wire` does not recognize and
the output bridge emits the same code `as_wire` round-trips. Choosing the trait
method over an `L: AsRef<str>` bound on `from_localized` keeps the wire code the
*explicit dedicated inverse* of `from_wire` — an `AsRef<str>` view could differ
from the wire code and silently desync the two directions.

`GqlLocale` is a **re-export of the featureless `br_core_values::LocaleCodec`** —
the codec is a value concern, not a transport concern, so it lives in the value
crate (no `async-graphql`, no `axum`). `into_localized` and `from_localized` bind
on `LocaleCodec` directly. The consequence is that a **serde-only `contract-*`
crate** can `impl GqlLocale (= LocaleCodec) for Locale` and reuse
`GqlLocalized::from_localized` **without** pulling the GraphQL/HTTP stack into the
published-language layer; the per-service `locale → wire` mapper collapses into
this one canonical codec. The edge keeps mapping the core's `locale_unknown` to
the `LOCALE_UNKNOWN` reason code (see `GqlValueError`).

**Field-naming.** `GqlLocalized.primaryLocale` holds a **locale code** (e.g.
`"en"`), not the primary text — named so deliberately, after a review surfaced
that a field literally named `primary` carrying a code misleads consumers. The
primary's text is the `content` of its matching entry in `entries`.

## async-graphql version coupling (the sensitive point)

This crate pins **`async-graphql = "7"`** — the estate-wide version
(`svc-notifier`, `svc-identity`, the be-botresources.ai workspace). Because the
GraphQL types here (`Affordance`, `Connection`, `SubscriptionPayload`, the
`GqlMoney*` / `GqlLocalized*` objects) appear in **every consumer's schema**,
this is a **shared-version coupling**: all consumers must agree on the same
`async-graphql` major. A bump is therefore a **coordinated migration across every
consumer**, gated in CI (`cargo-semver-checks`), not a local change — exactly the
shared-crate discipline the umbrella constitution describes for a breaking change
to a shared crate. Treat an `async-graphql` major bump as a breaking change to
this crate even when its own public surface is unchanged.

## Install

```toml
[dependencies]
br-util-graphql = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-graphql", tag = "v1.0.2", version = "1.0.2", features = ["graphql"] }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
