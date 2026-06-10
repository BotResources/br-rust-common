# Changelog — br-core-scope

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-06-10

**Added**
- Initial release. Pure contract types (tier `core`, no I/O, no `async`, no
  `br-util-*` dependency) for the **scope self-declaration** handshake — the
  shared language a declaring service and Identity both bind to.
  - `ScopeKey` — validated `{service}:{capability}` permission key. Intrinsic
    validation in the constructor (and re-run on deserialization, fail closed):
    ASCII `[a-z0-9_]`, exactly one `:` separating two non-empty segments, the
    `{service}` segment ≤ `SERVICE_KEY_MAX_LEN` (64) so it is always a *possible*
    `ServiceKey` (a scope no service could own is rejected), total length ≤
    `SCOPE_KEY_MAX_LEN` (128). **Dynamic, never a global enum.** No `Deref` —
    `as_str` / `AsRef<str>` / `service_segment` / `capability_segment`. The
    contextual "service segment must equal the declaring service" rule is **not**
    in the constructor: it is `is_owned_by(&ServiceKey)`, enforced at declaration
    assembly.
  - `ServiceKey` — validated `{service}` identifier (same charset, non-empty,
    ≤ `SERVICE_KEY_MAX_LEN` = 64). Serde re-validates.
  - `ScopeSpec { key, label_key, description_key, platform_only }` and
    `ServiceManifest { key, label_key, description_key }` — `label_key` /
    `description_key` are i18n keys, not rendered prose.
  - `ScopeDeclaration` — manifest + scope specs, validated **atomically** at
    construction (every scope's service segment matches the manifest's service
    key; no duplicate keys). Private fields read via `manifest()` / `scopes()`;
    deserialization re-runs the constructor so a malformed declaration fails
    closed.
  - `RawScopeDeclaration` (+ `RawServiceManifest`, `RawScopeSpec`) — the
    **receiver-side raw (unvalidated) wire form** of a declaration. A receiver
    deserializes it from any structurally well-formed payload (keys are plain
    strings, not yet validated) and calls `validate()` → `Result<ScopeDeclaration,
    ScopeDeclarationError>`, the explicit step that produces the *structured*
    rejection reason — so a malformed key yields `InvalidScopeKey { key,
    validation }` for a `ServiceScopesRejected` reply, never an opaque serde
    error / forced nak. Its serialized shape is byte-identical to
    `ScopeDeclaration`. `From<&ScopeDeclaration>` / `From<ScopeDeclaration>` give
    the sender path; it is a boundary artifact, never usable as a declaration.
  - `DeclareServiceScopes` — command payload carrying the declaration in its raw
    form (private field). `new(ScopeDeclaration)` is the sender's validated-only
    path (a well-behaved declarant can never send an invalid declaration);
    `validate()` is the receiver's explicit validation step; `raw()` gives
    read-only access. `ServiceScopesAccepted { service }` and
    `ServiceScopesRejected { service, reason }` complete the handshake. A consumer
    composes each as the `T` of a `br-core-integration` envelope; there is **no
    dependency** on `br-core-integration` (the envelope is generic over `T`),
    and correlation/causation/timestamps live on the envelope's `MessageMetadata`,
    never in these structs.
  - `KeyValidationError` — `Empty`, `TooLong { max, actual }`, `InvalidCharset`,
    `MalformedSegments`. The intrinsic-validation reason; also travels nested in
    a rejection, so it (de)serializes (internally tagged on `validation`).
  - `ScopeDeclarationError` — the **shared rejection-reason language**:
    `InvalidScopeKey`, `ScopePrefixMismatch`, `DuplicateScopeInDeclaration`
    (produced by the receiver-side `RawScopeDeclaration::validate` — the last two
    also by the local `ScopeDeclaration::new`) and `ScopeOwnedByAnotherService`
    (produced only by Identity's registry). One enum used on the receiver path
    and as the `ServiceScopesRejected` payload; (de)serializes (internally tagged
    on `reason`). Per codes-not-language, its `Display` strings are stable codes,
    not UI prose.
  - Deserializing a *bare* `ScopeKey` / `ServiceKey` / `ScopeSpec` /
    `ScopeDeclaration` re-runs validation and **fails closed with an opaque serde
    error** — deliberate: those are the validated types. The structured
    `InvalidScopeKey` reason is produced only on the raw-form validation path.
- **No `ServiceIdentityMismatch` / anti-spoof check** — the bus is auth-less
  behind a default-deny NetworkPolicy, so there is no authenticated principal to
  bind a declaration to; such a check would be a false guarantee and is
  deliberately absent.
