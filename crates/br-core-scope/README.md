# br-core-scope

Pure contract types for the **scope self-declaration** handshake: the shared
language both sides depend on when a service tells Identity which permission
scopes it owns. Tier `core` — no I/O, no transport, no `async`, and **no
dependency on any `br-util-*` crate** (nor on `br-core-integration`).

**Purpose.** A declaring service publishes a `DeclareServiceScopes` payload and
Identity replies `ServiceScopesAccepted` or `ServiceScopesRejected`. This crate
holds only the *shapes and their validation* — the same types build a
declaration locally (with intrinsic validation) and carry it over the bus.

**When to use.** A service declares scopes to Identity, or Identity's registry
processes a declaration; either side needs to agree on the wire shape and the
validation rules.

**When not to use.** The transport envelope, correlation, causation, and
timestamps — those are `br-core-integration`'s `IntegrationCommand<T>` /
`IntegrationEvent<T>` and their `EventMetadata`. This crate's messages are the
`T` they carry; it carries no envelope of its own.

## Key distinctions

- **Scope keys are dynamic, never a global enum.** Each service declares its own
  `{service}:{capability}` keys at runtime, so a fixed Rust enum could never
  enumerate them. `ScopeKey` carries the meaning in a validated newtype instead.
- **Intrinsic vs contextual validation.** Key *syntax* (charset, one `:`,
  non-empty segments, length) is intrinsic, enforced in the constructor and
  re-run on deserialization (fail closed). The rule "a scope's `{service}`
  segment must equal the declaring service's key" is **contextual** — it needs
  the declarant — so it is *not* in the constructor; check it with
  `ScopeKey::is_owned_by`, enforced when a `ScopeDeclaration` is assembled.
- **Receiver-side raw form.** A receiver must never be left re-publishing a
  declaration it can read. So `DeclareServiceScopes` carries the declaration in a
  **raw** (`RawScopeDeclaration`) form: a structurally well-formed payload — even
  one with a malformed key — always deserializes, and the receiver calls
  `validate()` to get either a validated `ScopeDeclaration` or the *structured*
  `ScopeDeclarationError` for a `ServiceScopesRejected` reply (never an opaque
  serde error). Senders build through the validated `ScopeDeclaration` only, so
  a well-behaved declarant can never put an invalid declaration on the wire. The
  raw form is a boundary artifact, never usable as a declaration: the sole path
  to a validated `ScopeDeclaration` is `validate()`.
- **One shared rejection language.** `ScopeDeclarationError` is used both on the
  receiver path (`InvalidScopeKey`, `ScopePrefixMismatch`,
  `DuplicateScopeInDeclaration`, via `RawScopeDeclaration::validate`) and by
  Identity's registry (which additionally produces `ScopeOwnedByAnotherService`,
  it alone). The two cross-cutting reasons are also produced by the local
  `ScopeDeclaration::new`. It is the payload of `ServiceScopesRejected`, so it
  (de)serializes; per codes-not-language its `Display` strings are stable codes,
  never UI prose.
- **Bare types deserialize strict / fail-closed.** Deserializing a bare
  `ScopeKey` / `ServiceKey` / `ScopeSpec` / `ScopeDeclaration` re-runs validation
  and fails closed with an **opaque serde error** — deliberate: those are the
  validated types. The *structured* `InvalidScopeKey` reason lives only on the
  raw-form validation path.
- **No anti-spoof check.** There is deliberately **no** `ServiceIdentityMismatch`:
  the bus is auth-less behind a default-deny NetworkPolicy, so there is no
  authenticated principal to bind a declaration to — shipping such a check would
  be a false guarantee.
- **No `Deref`.** Like the kernel id types, the newtypes expose `as_str()` /
  `AsRef<str>`, never `Deref`, so every raw access is explicit.

## What's inside

| Type | Role |
|---|---|
| `ScopeKey` | Validated `{service}:{capability}` permission key. `new` / `TryFrom<String>` validate ASCII `[a-z0-9_]`, exactly one `:`, non-empty segments, total ≤ `SCOPE_KEY_MAX_LEN` (128). `as_str` / `service_segment` / `capability_segment` / `is_owned_by(&ServiceKey)`. Serde re-validates on deserialize. |
| `ServiceKey` | Validated `{service}` identifier. Same charset, non-empty, ≤ `SERVICE_KEY_MAX_LEN` (64). `as_str` / `AsRef<str>`. Serde re-validates. |
| `ScopeSpec` | One declared scope: `key: ScopeKey`, `label_key`, `description_key` (i18n keys, not prose), `platform_only: bool`. |
| `ServiceManifest` | The declaring service's card: `key: ServiceKey`, `label_key`, `description_key`. |
| `ScopeDeclaration` | A manifest + its `ScopeSpec`s. `new` validates atomically (every scope owned by the manifest's service; no duplicate keys); deserialize re-runs it (fail closed, opaque serde error). Private fields, read via `manifest()` / `scopes()`. The validated type. |
| `RawScopeDeclaration` | The receiver-side raw (unvalidated) wire form: `RawServiceManifest` + `Vec<RawScopeSpec>`, keys as plain `String`. Always deserializes from structurally-valid JSON; `validate()` → `Result<ScopeDeclaration, ScopeDeclarationError>` (the explicit step that yields the structured `InvalidScopeKey`). `From<&ScopeDeclaration>` / `From<ScopeDeclaration>` for the sender path; wire shape byte-identical to `ScopeDeclaration`. A boundary artifact, never a declaration. |
| `KeyValidationError` | Why a key string is malformed: `Empty`, `TooLong { max, actual }`, `InvalidCharset`, `MalformedSegments`. `#[non_exhaustive]`; internally tagged on `validation`. |
| `ScopeDeclarationError` | Shared rejection language: `InvalidScopeKey`, `ScopePrefixMismatch`, `DuplicateScopeInDeclaration`, `ScopeOwnedByAnotherService`. `#[non_exhaustive]`; internally tagged on `reason`; (de)serializes (it is the `ServiceScopesRejected` payload). |
| `DeclareServiceScopes` | Command payload carrying the declaration in raw form (private). `new(ScopeDeclaration)` for the sender (validated-only); `validate()` → `Result<ScopeDeclaration, ScopeDeclarationError>` for the receiver; `raw()` for read-only inspection. Composed by a consumer as the `T` of an `IntegrationCommand`. |
| `ServiceScopesAccepted` | Accepted-event payload: `{ service: ServiceKey }`. Composed as the `T` of an `IntegrationEvent`. |
| `ServiceScopesRejected` | Rejected-event payload: `{ service: ServiceKey, reason: ScopeDeclarationError }` (one reason; rejection is atomic). |

Correlation, causation, and timestamps live on the integration envelope's
`EventMetadata`, **never** in these payloads — and that envelope is composed at
the consumer (`br-core-integration`'s `IntegrationCommand<T>` /
`IntegrationEvent<T>`); this crate has **no dependency** on it.

## Usage

```rust
use br_core_scope::{
    DeclareServiceScopes, ScopeDeclaration, ScopeKey, ScopeSpec, ServiceKey,
    ServiceManifest,
};

// Build the declaring service's manifest and the scopes it owns. Each key is
// validated as it is built.
let manifest = ServiceManifest::new(
    ServiceKey::new("notifier").unwrap(),
    "service.notifier.label",
    "service.notifier.description",
);
let scopes = vec![
    ScopeSpec::new(ScopeKey::new("notifier:read").unwrap(), "scope.read.label", "scope.read.desc", false),
    ScopeSpec::new(ScopeKey::new("notifier:admin").unwrap(), "scope.admin.label", "scope.admin.desc", true),
];

// Atomic validation: every scope must be owned by `notifier`, no duplicates.
let declaration = ScopeDeclaration::new(manifest, scopes).expect("valid declaration");

// Wrap as the declare-command payload; an `IntegrationCommand<DeclareServiceScopes>`
// (from br-core-integration, composed at the consumer) carries it on
// `identity.cmd.service_scope.declare.v1`. On the wire the declaration travels in
// its raw form.
let payload = DeclareServiceScopes::new(declaration);

// Receiver side (Identity): deserialize the payload, then validate. A malformed
// key yields a structured `ScopeDeclarationError` for a `ServiceScopesRejected`
// reply — never an unreadable nak.
// let cmd: DeclareServiceScopes = serde_json::from_slice(bytes)?;
// match cmd.validate() {
//     Ok(declaration) => { /* check against the registry, then accept */ }
//     Err(reason)     => { /* reply ServiceScopesRejected { service, reason } */ }
// }
```

## Install

```toml
[dependencies]
br-core-scope = { git = "https://github.com/BotResources/br-rust-common", package = "br-core-scope", tag = "v1.0.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
