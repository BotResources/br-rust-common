# Changelog — br-util-observability

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.0] — 2026-06-12

**Added**
- Initial release. One boot-time observability setup for every BR process
  (tier `util`, a thin technical wrapper — no domain, no policy).
  - `init_logging(component)` — installs a global `tracing` subscriber that
    emits **one JSON object per line** on stdout, with the canonical keys `ts`
    (RFC 3339, UTC), `level`, `component`, `msg`, plus every event field. Level
    is env-driven (`RUST_LOG`, default `info`). Idempotent: a second call logs a
    notice to stderr and returns instead of panicking, so a double-init or a
    test harness is safe.
  - `liveness_route::<S>()` — an Axum `GET` route that **always** answers
    `200 OK` (body `"alive"`), generic over the router state so it mounts into
    any `Router<S>`. **Liveness, not readiness:** a failed liveness probe means
    Kubernetes restarts the process, so it is unconditional by design;
    readiness (out-of-rotation without restart) stays in `br-util-axum-readiness`
    (`/readyz`). The two crates are complementary, never overlapping.

**Unified from the seed (issue #45)**
- The JSON formatter was duplicated **verbatim** across the gateway's
  `composer` and `fe-syncer` binaries — byte-identical except the hard-coded
  `COMPONENT` constant. Unified into one reusable `init_logging(component)`, the
  component name being the only thing that varied.

**Hardening vs the seed**
- **Non-finite floats no longer fabricate a value.** The seed coerced NaN /
  Infinity to `0` (JSON has no NaN/Infinity); the value now renders as `null`,
  preserving the field without lying about its magnitude.
- **The canonical keys are guarded.** A user field literally named `ts` /
  `level` / `component` / `msg` is dropped so it can never clobber the line's
  canonical value (carried over from the seed and tested explicitly).
- **`ts` has fixed-width microsecond precision** (`….123456+00:00`). The
  formatter previously used `to_rfc3339()`, which emits a *variable*-width
  fraction (none at all when the sub-second part is zero) — breaking a consumer
  parsing `ts` with a fixed-width regex against the shape the README documents.
  It now uses `to_rfc3339_opts(SecondsFormat::Micros, false)`, pinning six
  fractional digits and the documented numeric `+00:00` offset. Pinned by a unit
  test on the line shape.
- **An explicit `message` field can no longer silently drop the real message.**
  `tracing` records the format-string message under a field named `message`; a
  caller passing an explicit `message = …` field made the event carry the name
  twice. The visitor now lifts the **first** `message` (the format-string
  message) to `msg` and keeps any later `message`-named field as an ordinary
  `message` field — nothing is lost and `msg` is never clobbered. Covered by a
  collision unit test.
- **The line-builder is a pure, subscriber-free function**, unit-tested
  directly (valid JSON, canonical keys present, user fields carried, no
  clobbering, fixed `ts` shape), with the `record_*` field paths (including the
  `message` collision) covered by a capturing-layer test.

**Scope notes**
- **Metrics are intentionally out of v0.1.0.** No BR process exposes metrics
  yet; pulling a metrics client into a load-bearing tier-`util` crate would
  force a shared-version coupling on every consumer for an unused surface. A
  `/metrics` endpoint earns its own design when a real need lands.
