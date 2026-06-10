# Changelog — br-util-axum-readiness

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.1.1] — 2026-06-10

**Fixed**
- The gate no longer panics on a **poisoned lock**. `snapshot`, `set_ready`,
  and `set_not_ready` previously did `.expect("readiness lock poisoned")`, so
  if any writer ever panicked while holding the guard, every subsequent
  `/readyz` read would panic — the readiness probe itself would 500/abort
  instead of reporting a real state. All three paths now recover the poisoned
  guard (`unwrap_or_else(|e| e.into_inner())`). This is safe for this type:
  `Readiness` is a plain enum and each mutation is a single infallible
  assignment with no I/O, so a panic cannot leave it half-written — there is no
  torn value to protect against. A readiness gate's own failure mode must fail
  **closed** (keep answering with a real up/down state), never abort the probe.
  A test poisons the lock (a thread panics mid-write) and asserts `/readyz`
  still answers.

## [0.1.0] — 2026-06-01

**Added**
- Initial release. A readiness gate for HTTP services: `ReadinessHandle` (a
  cloneable UP/DOWN toggle carrying an operator-facing reason) and
  `readiness_route`, an Axum `GET` route returning `200 OK` (body `"ready"`)
  when ready and `503 Service Unavailable` (body = the reason) otherwise,
  generic over the router state so it mounts into any `Router<S>`. **Readiness,
  not liveness** — a not-ready service is taken out of rotation without being
  restarted; this crate intentionally ships no liveness probe. What gates
  readiness is the caller's concern. State transitions are logged via `tracing`
  (UP/DOWN, with the reason), deduplicated so a repeated state never spams the
  log.
