# Changelog — br-util-broadcast

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-06-12

**Added**
- Initial release. In-process **event bus** (tier `util`) for **post-commit
  fan-out** of domain events to same-process GraphQL subscriptions — a
  domain-free wrapper over a `tokio::sync::broadcast` channel, generic over the
  payload `T: Clone`. Depends only on `tokio` and `thiserror` (no `br-*`
  dependency); this is the in-process real-time fan-out, **not** the
  inter-service NATS bus.
  - `EventBus<T>` — cloneable bus handle. `new(capacity)`, `subscribe()`,
    `subscriber_count()`, and the **single** publish path
    `publish_after_commit(PendingBroadcast<T>) -> Result<(), BroadcastError>`.
    There is **no method that takes a bare event**, by design.
  - `PendingBroadcast<T>` — the post-commit buffer a command fills while it runs.
    It **carries no channel**: there is no way for a staged event to reach a
    subscriber except by handing the buffer to `publish_after_commit`.
    `new()` / `from_events(Vec<T>)` / `push` / `extend` / `len` / `is_empty`,
    plus `Default`, `Extend`, and `FromIterator`.
  - `BroadcastError` — the crate's own error type. Per codes-not-language its
    `#[error(...)]` string is a **stable code** (`no_subscribers unheard=N`),
    never UI prose. `#[non_exhaustive]`. The single `NoSubscribers { unheard }`
    variant is an **informational** signal (the events are already committed and
    durable when fan-out runs), never a persistence failure.

**Notify-after-commit, baked into the API shape (issue #44, be-botresources.ai#66)**
- The seed (`be-botresources.ai crates/infra/src/event_bus.rs`) exposed a bare
  `send(&event)` reachable from anywhere — including *inside* a transaction,
  which is the #66 bug (a later rollback leaves subscribers having observed state
  that never persisted). This crate removes that footgun: the buffer
  (`PendingBroadcast`, built during the command) and the channel (reachable only
  via the named `publish_after_commit`) are **structurally distinct**, so the
  correct order — *commit, then notify* — is the only order the API can express.
- **Generic over the payload** (`T: Clone`) instead of the seed's hardcoded
  `DomainEvent`, keeping the crate tier-`util` and domain-free.
- **The no-listener case is surfaced**, not silently swallowed: the seed's
  `let _ = self.sender.send(...)` dropped the "nobody heard it" signal;
  `publish_after_commit` returns `BroadcastError::NoSubscribers { unheard }` so a
  caller may log/meter it (and may still ignore it — the events are durable).
- **Behavioural specs** (Given/When/Then) cover multi-subscriber fan-out and
  ordering, the lagged and closed-receiver behaviour, the no-subscribers signal,
  the empty-buffer no-op, channel sharing across `clone`, and — the load-bearing
  one — that a `PendingBroadcast` built pre-commit delivers **nothing** until it
  is handed to `publish_after_commit`.
