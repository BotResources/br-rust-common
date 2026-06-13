# Changelog — br-util-observability

All notable changes to this crate are documented in this file. Format inspired
by [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); the crate follows
[SemVer](https://semver.org/).

## [0.2.0] — 2026-06-13

**Added**
- A generic, anonymized Prometheus metrics capability — the exact twin of the
  v0.1.0 logging + liveness surface (mechanism in the lib, the service declares
  its own domain metrics). Built on the `metrics` facade so the recorder is
  process-global and the service emits with `metrics::{counter,gauge,histogram}`
  against the same recorder, no handle to thread through.
  - `init_metrics(component) -> Result<MetricsHandle, MetricsError>` — installs
    the process-global Prometheus recorder (via `PrometheusBuilder::install_recorder`),
    registers the universal process collectors, pins the latency buckets **for its
    own `http_request_duration_seconds` metric only** (`set_buckets_for_metric`,
    leaving the recorder default neutral so the service's own histograms keep their
    own buckets), and adds `component` as a constant global label (symmetric with
    `init_logging`). Fallible: the recorder installs once per process, so a
    double-install returns `MetricsError::Install` instead of panicking (unlike
    logging's idempotent global subscriber — a recorder double-install is a real
    error worth surfacing).
  - `MetricsHandle` (`Clone`) — `render()` produces the Prometheus text
    exposition on demand; it refreshes the process collectors and runs recorder
    upkeep on each call, so the scrape itself is the only refresh trigger (no
    background task, no polling — aligns with the platform's never-poll stance).
    `prometheus()` exposes the underlying `PrometheusHandle`.
  - `metrics_route::<S>(MetricsHandle)` — an Axum `GET` route serving the
    exposition (`200 OK`, content-type `text/plain; version=0.0.4`), generic over
    the router state, mirroring `liveness_route`.
  - `http_metrics_layer() -> HttpMetricsLayer` — a tower `Layer` recording
    `http_requests_total` (counter), `http_request_duration_seconds` (histogram,
    buckets `5ms..10s`), and `http_requests_in_flight` (gauge). The in-flight gauge
    is decremented on `Drop` (RAII), so an error, a panic, or a cancelled request
    future cannot leak it upward; the counter and histogram record only on
    successful completion, as they count completed requests. Proved by test.
  - `MetricsError` (`thiserror`, `#[non_exhaustive]`) — `Buckets` / `Install`,
    stable `Display` codes; the crate owns its error type.
  - Universal process collectors (CPU, resident/virtual memory, open/max FDs,
    process start time, thread count) via `metrics-process`.

**Anonymization (EU/GDPR — treated as a security property, proved by test)**
- HTTP requests are labeled by **method + matched-route template + status code
  only**. The route label is Axum's `MatchedPath` template (`/users/{id}`),
  **never** the concrete path (`/users/123`). When there is no matched route the
  label **fails closed** to the constant sentinel `"<unmatched>"` — it never
  falls back to the raw path (which would be unbounded, user-controlled
  cardinality and a PII leak). Every label is a small, closed, bounded set; the
  exposition exposes only aggregates, never per-request rows. A dedicated test
  asserts a request to `/users/12345` produces `route="/users/{id}"` and that the
  concrete value `12345` appears **nowhere** in the rendered exposition; a second
  asserts an unmatched path yields the sentinel and its raw value leaks nowhere.

**Dependency choices**
- `metrics` (facade) + `metrics-exporter-prometheus` + `metrics-process`. The
  facade lets a service register its own metrics with zero coupling to this
  crate's types — the cardinal "mechanism in the lib, policy in the project"
  rule. `PrometheusHandle::render()` fits an on-demand Axum `/metrics` route, so
  `metrics-exporter-prometheus` is pulled with `default-features = false`: its
  default `http-listener` feature would spin up a **second** `hyper` HTTP server
  on `:9000`, which we never use (we serve from the service's own Axum router).
  `install_recorder` and `set_buckets` are not feature-gated, so the recorder is
  fully usable without the listener. Deps are deliberate and pinned (no
  Dependabot in this org).

**Deferred (deliberate)**
- **Tokio runtime metrics are not included.** They require building every
  consumer with `--cfg tokio_unstable` (a global `RUSTFLAGS` cfg), which a
  load-bearing tier-`util` crate must not force on its consumers. They will be
  added behind an off-by-default cargo feature if and when a consumer opts into
  `tokio_unstable`.

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
