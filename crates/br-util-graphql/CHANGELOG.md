# Changelog — br-util-graphql

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-06-12

**Added**
- Initial release. The GraphQL/REST edge kit (tier `util` — types / functions /
  traits, no domain). Entire surface behind the `graphql` feature (off by
  default); enabling it pulls `async-graphql` + `axum` + `br-core-values`. With
  no features the crate compiles to nothing.
  - **3-layer error mapping** — `ErrorCode`, the canonical cross-service code set
    (`UNAUTHENTICATED` / `FORBIDDEN` / `NOT_FOUND` / `CONFLICT` / `BAD_USER_INPUT`
    / `INVALID_STATE` / `INTERNAL`), with a stable wire string and REST HTTP
    status per variant. `EdgeError` — the application-layer error built from a
    service's domain error, carrying the class code + an optional precise
    `reason_code` (the broken domain rule) + structured `params` + a
    never-returned internal `detail`. Two render edges: `into_gql()` (an
    `async_graphql::Error` with `extensions.code` / `reason` / `params.*`, plus
    `From<EdgeError>`) and `IntoResponse` (the mirrored REST JSON body
    `{ "error": { code, reason?, params? } }`). An `Internal` fault logs its
    detail and returns only the code — SQL / serialization / panic text never
    reaches a client. **Unifies the ~six diverging `svc-*/src/error.rs` copies
    across the estate** into one source of truth; the code set is documented in
    the README as the authoritative frontend contract.
  - **`Affordance { action, allowed, reason_code }`** — the dumb-frontend
    contract. `allow(action)` / `block(action, reason_code)` constructors make a
    blocked-without-reason affordance unrepresentable (no silent denial);
    `reason_code` is a stable key, never UI prose.
  - **`MutationResult`** — the success ack (`{ success: true }`). The type has no
    field to smuggle a DTO back, locking R1 (a mutation returns an ack or an
    `EdgeError`, never the mutated state) into the schema.
  - **`Connection<T>` / `Edge<T>` / `PageInfo`** — reusable cursor pagination in
    the Relay-flavoured shape (`edges { node cursor }` + `pageInfo`); generic
    over any `T: OutputType` via `#[derive(SimpleObject)]` + `#[graphql(name_type)]`,
    the per-node GraphQL name woven from the node through a `TypeName` impl
    (`{Node}Connection` / `{Node}Edge`) so two node types do not collide in one
    schema. `Connection::forward` derives the boundary cursors. Cursor is an
    opaque `String`; the crate prescribes no encoding. Generalizes the
    per-service connection (e.g. svc-notifier's `{ nodes, has_next_page }`).
  - **`SubscriptionPayload<E, T>`** — the collaborative-pure push: the domain
    `event`, the fresh `entity` it produced, and the `affordances` recalculated
    for the subscriber, so a client folds it into state without a refetch.
    Generic over the event union `E` and the entity `T` (both `OutputType`).
  - **Fallible VO wrappers (`values`)** over `br-core-values` — `TryFrom` / seam
    conversions that **fail with a typed code, never coerce** (the inverse of the
    Hanshow seed). `GqlMoney`/`GqlMoneyInput` carry the **full `i64`** minor-unit
    amount end-to-end through a dedicated `MoneyAmount` scalar — a **decimal
    string** on the wire (e.g. `"123456789012"`), never the built-in 32-bit `Int`
    and never a numeric scalar (JSON numbers lose precision above 2⁵³), the
    GitHub/Stripe-style JS-safe money representation. Output is infallible (no
    ceiling, no truncation); `MONEY_OUT_OF_RANGE` fires only when an inbound
    string is non-numeric or overflows `i64`. The currency is validated, never
    coerced. `GqlLocalizedInput` (refuse `LOCALE_UNKNOWN` and `PRIMARY_CONTENT_MISSING`);
    `GqlLocale` (the product-supplied seam parsing a wire locale into its closed
    `Locale` enum, refusing unknowns). `GqlValueError` carries the rejection and
    converts to a `BAD_USER_INPUT` `EdgeError` with the precise reason + param; a
    value-object rejection that is none of the three named codes passes its own
    stable code through (`ValueRejected`).

**Hardening vs the seed implementations (epic #38, issue #40)**
- The error mapping is **unified** (was ~six diverging `svc-*/src/error.rs`
  copies) and the code set is **documented as the cross-service contract** the
  frontends bind to.
- The VO wrappers are **fallible** (`TryFrom` with typed codes), the exact
  inverse of the Hanshow wrappers — which coerced unknown locales to the default,
  accepted missing primary content, and **truncated** `Money` i64→i32. We do the
  opposite: the full `i64` is carried as a decimal string (no ceiling, no
  truncation), with `MONEY_OUT_OF_RANGE` repurposed for the inbound parse/overflow
  boundary; `LOCALE_UNKNOWN` refuses the coercion; `PRIMARY_CONTENT_MISSING`
  refuses the missing primary.
- `Connection<T>` and `SubscriptionPayload<E, T>` are **generic and reusable**
  (were per-service, hand-rolled types).

**Version coupling**
- Pins `async-graphql = "7"` (the estate-wide version). Because the GraphQL types
  here appear in every consumer's schema, this is a **shared-version coupling**:
  a bump is a coordinated migration across all consumers, gated in CI — not a
  local change. Documented in the README.
