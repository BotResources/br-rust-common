# Changelog — br-core-events

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.4.0] — 2026-06-10

Breaks the **Rust API** while keeping the **JSON wire format
backward-compatible**: payloads written before this version (no `actor_kind`)
still deserialize. Only the in-process Rust types changed.

**Changed (BREAKING)**
- `EventMetadata.actor_id: Uuid` is replaced by `actor: br_core_kernel::Actor`,
  a typed human-or-machine identity (new dependency on `br-core-kernel`). A
  bare `Uuid` could not distinguish a human user from a service account; the
  typed `Actor` carries that distinction.
  - **Wire contract.** Serialization emits a flat object with `actor_id` (uuid
    string) **and** a new `actor_kind` (`"human"` | `"service"`), alongside
    `correlation_id` and the optional `causation_id` exactly as before.
    Deserialization reads `actor_id` and an **optional** `actor_kind`.
  - **Legacy-defaults-to-Human policy.** When `actor_kind` is **absent** —
    every payload written before 0.4.0 — the actor defaults to `Actor::Human`.
    Machine actors did not exist in this envelope before this version, so a
    human is the only shape a legacy payload could have carried. An explicit
    `"actor_kind": null` is treated as absent (no real producer emits it). An
    **unknown** `actor_kind` value (anything other than `"human"`/`"service"`)
    is a hard deserialization error: it fails closed rather than guessing.
- `EventMetadata`, `RawEvent`, and `DomainEvent` are now `#[non_exhaustive]`.
  Struct-literal construction from outside the crate is no longer possible;
  use the constructors. Fields stay `pub` for read access; cross-crate pattern
  matches must include a `..` rest pattern (a `#[non_exhaustive]` consequence).

- `RawEvent`'s field and `RawEvent::new` argument order now lead with
  `aggregate_id` then `aggregate_type`, matching the shared fields of
  `DomainEvent` so the producer-side and persisted types read in parallel.
  `RawEvent` is producer-side only and not `Serialize`/`Deserialize`, so there
  is no wire impact; callers of `RawEvent::new` swap the first two arguments.

**Added**
- Constructors: `EventMetadata::new(actor, correlation_id)` (causation `None`)
  + builder-style `EventMetadata::with_causation(causation_id)`;
  `RawEvent::new(aggregate_id, aggregate_type, event_type, payload)`;
  `DomainEvent::new(id, aggregate_id, aggregate_type, event_type, payload, metadata, occurred_at)`.
- `Actor`, `UserId`, `ServiceAccountId` re-exported at the crate root so
  consumers can construct metadata without adding a direct `br-core-kernel`
  dependency.

**Migration**

Struct literals → constructors:

```rust
// before (0.3.x)
let meta = EventMetadata {
    actor_id: user_id,            // a bare Uuid
    correlation_id: req_id,
    causation_id: Some(cause_id),
};

// after (0.4.0)
use br_core_events::{Actor, UserId};
let meta = EventMetadata::new(Actor::Human(UserId::from(user_id)), req_id)
    .with_causation(cause_id);
```

Reading the actor's uuid:

```rust
// before: metadata.actor_id
// after:  metadata.actor.id()          // the inner uuid, either variant
//         metadata.actor.is_service()  // branch on kind when it matters
```

## [0.3.1] — 2026-05-22

**Changed**
- Workspace metadata cleanup: `edition`, `rust-version`, `license`, and
  `repository` now inherit from `[workspace.package]` via
  `.workspace = true`. The crate's `rust-version` was previously declared as
  `1.85` per-crate while the workspace, CI, and top-level README all
  advertised `1.88`; the inherited value is now consistently `1.88`. No API
  or runtime behavior change.

## [0.3.0] — 2026-04-17

- Carved out of `br-service-core` during the workspace split into
  `br-rust-common`. Provides shared event types (`EventMetadata`,
  `RawEvent`, `DomainEvent`).
