# Changelog — br-util-axum-readiness

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

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
