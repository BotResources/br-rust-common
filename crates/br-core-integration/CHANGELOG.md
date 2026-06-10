# Changelog — br-core-integration

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.2.0] — 2026-06-10

Breaks the **Rust API**; the **JSON wire format stays backward-compatible** —
the metadata change is inherited from `br-core-events` 0.4.0, which keeps
pre-`Actor` payloads (no `actor_kind`) deserializing (defaulting to a human
actor). Internal module layout was split, but **all public import paths are
unchanged** (everything is still importable from the crate root).

**Changed (BREAKING)**
- `MessageMetadata` is now a re-export of `br_core_events::EventMetadata`
  (`pub use … as MessageMetadata`), not a hand-synced local duplicate. The
  public name is unchanged, but the type now carries a typed
  `br_core_kernel::Actor` instead of `actor_id: Uuid`, and a new dependency on
  `br-core-events`. It inherits that crate's wire contract and its
  legacy-defaults-to-human guarantee. *Migration:* `metadata.actor_id` →
  `metadata.actor.id()`; construct via `MessageMetadata::new(actor, correlation_id)`.
- `IntegrationError::Publish(String)` → `Publish { kind: PublishErrorKind, detail: String }`.
  `kind` classifies the failure (`NoStream`, `Timeout`, `Other`) so callers can
  branch without parsing a string. `NoStream` is the production-meaningful
  case: a publish to a subject no stream captures (a missing declared stream).
- `IntegrationEvent<T>`, `IntegrationCommand<T>`, and `IntegrationError` are now
  `#[non_exhaustive]`. Struct-literal construction of the envelopes from outside
  the crate is no longer possible; use the constructors. Match `IntegrationError`
  and `PublishErrorKind` with a wildcard arm.

**Added**
- `PublishErrorKind` enum (`#[non_exhaustive]`): `NoStream`, `Timeout`, `Other`,
  classified honestly from async-nats 0.48's own `PublishErrorKind`. Ambiguous
  kinds — including `BrokenPipe` — map to `Other` rather than claiming a cause
  the transport did not report.
- Constructors: `IntegrationEvent::new(...)`, `IntegrationCommand::new(...)`.
- `integration_subject(bc, kind, aggregate, name, version)` + `MessageKind`
  (`Cmd`/`Evt`) + `SubjectError` — builds `{bc}.{cmd|evt}.{aggregate}.{name}.v{N}`
  and validates each segment is non-empty and drawn from `[a-z0-9-]` (no `.`,
  no NATS wildcards `*`/`>`, no whitespace — a malformed segment can neither
  break the subject structure nor collide with subscriber wildcards). The
  subject convention now lives in this helper instead of being repeated as prose.
- `Actor`, `UserId`, `ServiceAccountId` re-exported at the crate root (via
  `br-core-events`) so consumers can construct `MessageMetadata` without adding
  a direct `br-core-kernel` dependency.

**Docs**
- Documented why the envelopes' `version` field is `u8`: a published-contract
  schema version is bumped only on a breaking payload change and never
  realistically exceeds 255, so a wider integer would be dead range on the
  wire. No code change.
- E2E `tests/nats.rs` teardown cleanup: removed a redundant `delete_message(1)`
  no-op that ran just before deleting the whole stream, and teardown failures
  now `eprintln!` instead of being fully silenced — a leaked stream can capture
  a later test's messages, so a cleanup failure should be diagnosable.

**Migration**

Struct literals → constructors, and the new `Publish` shape:

```rust
// before (0.1.0)
let evt = IntegrationEvent {
    event_id, event_type: "user.created".into(), version: 1,
    occurred_at, metadata, payload,
};
let meta = MessageMetadata { actor_id, correlation_id, causation_id: None };
match err { IntegrationError::Publish(s) => /* string */ }

// after (0.2.0)
use br_core_integration::{Actor, UserId};
let evt = IntegrationEvent::new(event_id, "user.created", 1, occurred_at, metadata, payload);
let meta = MessageMetadata::new(Actor::Human(UserId::from(actor_id)), correlation_id);
match err { IntegrationError::Publish { kind, detail } => /* branch on kind */, _ => {} }
```

Reading the actor's uuid: `metadata.actor_id` → `metadata.actor.id()`.

## [0.1.0] — 2026-05-22

Initial release. Provides:

- `MessageMetadata`, `IntegrationEvent<T>`, `IntegrationCommand<T>` envelopes.
- `IntegrationError` (publish / serialization).
- Object-safe `IntegrationPublisher` trait with `publish` and
  `publish_if_connected` methods.
- `IntegrationPublisherExt` blanket helpers for typed publishing
  (`publish_event`, `publish_command`, `_if_connected` variants).
- `NatsIntegrationPublisher` (JetStream, awaits broker ack on `publish`,
  logs and swallows errors on `publish_if_connected`).
- `NoopIntegrationPublisher` for tests.
