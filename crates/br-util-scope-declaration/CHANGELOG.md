# Changelog — br-util-scope-declaration

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.1] — 2026-06-10

**Docs**
- The `README.md` is now the crate's root doc (`#![doc = include_str!(..)]`), so
  its usage example is compiled and run as a doctest by `cargo test` — the README
  can no longer drift from the code. The `lib.rs` module docs keep only the
  rustdoc cross-links the README leaves to the reference, no longer a hand-synced
  duplicate of the README example and prose.
- Fixed the README usage example so it compiles: it now wraps the snippet in an
  `async` boot function (the `?` operator and the `.await` need a fallible async
  context) and matches the `#[non_exhaustive]` `ScopeDeclarationOutcome`
  additively. The previous example referenced `jetstream` / `readiness` with no
  binding and omitted the wildcard arm — it would not have compiled, but nothing
  compiled it.

## [0.1.0] — 2026-06-10

**Added**
- Initial release. `declare_scopes` — the boot-time **scope-declaration
  handshake** helper: a generic service that owns scopes declares them to
  Identity at startup and gates its readiness on the confirmation, in a few
  lines.
  - **Subscribe-first / re-publish-on-timeout protocol.** Generate
    `correlation_id = C` once; create a per-replica, per-boot
    `CorrelatedAwaiter` (never durable, never a queue-group) over both
    confirmation subjects *before* publishing the durable declare command
    (`IntegrationCommand<DeclareServiceScopes>`, `metadata.correlation_id = C`);
    await the correlated confirmation; on a wait timeout, **re-publish (same
    `C`)** and keep awaiting indefinitely (Identity may be down; the readiness
    gate keeps the pod out of rotation meanwhile — an accepted coupling).
    **Duplicate confirmations are expected and harmless** — first correlated
    match wins.
  - **Outcomes drive `br-util-axum-readiness`.** Accepted → readiness **UP**,
    returns `Accepted`. Rejected → readiness **DOWN** + `tracing::error` with the
    structured reason (codes, not prose), **no retry** (rejection is
    deterministic), returns `Rejected(ServiceScopesRejected)` for the caller to
    act on. Both also returned, so the caller decides whether to stay alive out
    of rotation or exit.
  - **Disabled mode** (`ScopeDeclarationConfig::enabled == false`, wired from
    Helm): skips the handshake entirely — no awaiter, no publish — sets readiness
    **UP**, returns `Disabled`. Distinct from the intrinsic *scopeless* case (a
    service owning no scopes does not call this helper at all).
  - **Declaring-service actor** (`declaring_actor`): a deterministic,
    name-based service-account id (`uuid_v5` over the service key under a fixed
    crate namespace) stamped on `metadata.actor`. It is *declarative provenance*
    — which service authored the declaration, by convention — and
    **authenticates nothing** (the boot bus has no authenticated principal).
  - **Fail-loud declared infrastructure.** The JetStream stream is
    pre-declared: the awaiter binds it by name and the helper returns
    `IntegrationError::Consume { NoStream }` if it is missing — it never creates
    a stream or a durable.
  - E2E against a real NATS JetStream broker, with a **stub receiver** standing
    in for Identity: Accepted / Rejected / timeout→re-publish→Accepted /
    disabled (asserts no publish) / duplicate-confirmation (first match wins) /
    missing-stream (fail loud), each asserting the readiness gate state.

**Notes**
- Subjects are built with `br_core_integration::integration_subject` (which
  accepts snake_case segments as of `br-core-integration` 0.3.0) and pinned to
  the canonical contract strings by a unit test.
