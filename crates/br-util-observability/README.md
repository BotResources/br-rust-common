# br-util-observability

One boot-time observability setup for every BotResources process: structured
JSON logging, an always-200 `/livez` liveness route, and an anonymized
Prometheus `/metrics` endpoint. Tier `util`; it enforces no domain policy.

## A BR platform convention, not a neutral utility

This crate is **opinionated by design**: it is the BotResources house observability
stack — Axum + `tracing`-JSON + Prometheus + an unconditional `/livez` + universal
process/HTTP collectors — packaged so every BR process boots the *same* shape.
It is **not** a vendor-neutral abstraction and does not try to be: it bakes in the
JSON log schema, the liveness-vs-readiness split, the EU/GDPR label-anonymization
rule, and the `metrics` + `metrics-exporter-prometheus` backend as deliberate
platform choices. Adopt it to inherit the convention wholesale; if a service needs
a fundamentally different observability backend, it wires that itself rather than
bending this crate.

**`MetricsHandle::prometheus() -> &PrometheusHandle` is an intentional part of the
public contract**, not a leak. Because the chosen backend *is* the convention,
exposing the underlying `PrometheusHandle` lets a service reach the recorder
directly when it needs to (custom rendering, additional bucket configuration via
`set_buckets_for_metric`, recorder upkeep) without this crate having to proxy every
`metrics-exporter-prometheus` capability. It ties the public surface to that crate
on purpose — a backend swap is a breaking change here by design, consistent with
the convention this crate exists to enforce.

## What's inside

| Item | Kind | Behavior |
|---|---|---|
| `init_logging` | `fn(component: &str)` | Installs a global `tracing` subscriber that emits **one JSON object per line** on stdout. Canonical keys: `ts` (RFC 3339, UTC), `level`, `component`, `msg`; every event field is carried alongside. Level is env-driven (`RUST_LOG`, default `info`). Idempotent — a second call is a no-op (logs a notice, never panics). Call once, first thing in `main`. |
| `liveness_route::<S>` | `fn() -> MethodRouter<S>` | Axum `GET` route, **always** `200 OK` (body `"alive"`). Generic over the router state, so it mounts into any `Router<S>`. |
| `init_metrics` | `fn(component: &str) -> Result<MetricsHandle, MetricsError>` | Installs the **process-global** Prometheus recorder, registers the universal process collectors, and pins the latency buckets **for its own `http_request_duration_seconds` metric only** (the recorder default stays neutral, so the service's own histograms are unaffected). `component` is a constant global label (the service name, never PII), symmetric with `init_logging`. Fallible (the recorder installs once per process); a second call returns `MetricsError::Install` rather than panicking. Call once in `main`, keep the handle. |
| `MetricsHandle` | `struct` (`Clone`) | Renders the Prometheus text exposition on demand (`render()`); refreshes the process collectors and runs recorder upkeep on each render. `prometheus()` exposes the underlying `PrometheusHandle`. Mechanism only — the service registers and updates **its own** domain metrics through the global `metrics::{counter,gauge,histogram}` macros against the same recorder. |
| `metrics_route::<S>` | `fn(MetricsHandle) -> MethodRouter<S>` | Axum `GET` route that serves the exposition: `200 OK`, content-type `text/plain; version=0.0.4`. Generic over the router state, mirrors `liveness_route`. |
| `http_metrics_layer` | `fn() -> HttpMetricsLayer` | A tower `Layer` that records `http_requests_total` (counter) and `http_request_duration_seconds` (histogram) labeled by **method + matched-route template + status**, plus `http_requests_in_flight` (gauge) labeled by **method + matched-route template** (status is unknown until the response is produced). |
| `MetricsError` | `enum` (`thiserror`, `#[non_exhaustive]`) | `Buckets` / `Install`, stable `Display` codes. |

## Logging

Each line is a self-contained JSON object:

```json
{"ts":"2026-06-12T10:00:00.123456+00:00","level":"INFO","component":"composer","msg":"started","port":8080}
```

- **`component`** is the only thing that varies between processes, so it is the
  sole parameter. The formatter is otherwise identical everywhere.
- **Canonical keys can't be clobbered.** An event field literally named `ts`,
  `level`, `component`, or `msg` is dropped — it can never overwrite the line's
  canonical value.
- **Non-finite floats render as `null`, not a fake `0`.** JSON has no NaN /
  Infinity; the field is preserved without lying about its magnitude.
- **Stdout only.** Log shipping is the platform's concern (the container's
  stdout is collected); the process does not own a sink or a file.

## Liveness, not readiness — complementary to `br-util-axum-readiness`

The two probes answer different questions and drive different orchestrator
actions. This crate owns **liveness**; `br-util-axum-readiness` owns
**readiness**. There is no overlap — wire both.

| Probe | Crate | Question | A failure means |
|---|---|---|---|
| `/livez` | **this crate** | is the process alive? | Kubernetes **restarts** it |
| `/readyz` | `br-util-axum-readiness` | should it receive traffic *now*? | taken **out of rotation**, not killed |

Liveness is **unconditionally** `200` by design: gating it on a dependency would
turn a transient outage into a crash-loop. Never point a liveness probe at
`/readyz`.

## Metrics

The lib ships the **mechanism**; the consuming service declares its **policy**
(its own domain metrics). `init_metrics` installs one process-global Prometheus
recorder, and everything emits against it:

- **Universal process collectors** — CPU, resident/virtual memory, open/max file
  descriptors, process start time and thread count (`process_*`), via
  `metrics-process` (cross-platform; on Linux/K8s it reads `/proc`).
- **HTTP collectors** (`http_metrics_layer`) — `http_requests_total` (counter),
  `http_request_duration_seconds` (histogram), `http_requests_in_flight` (gauge).
- **The service's own metrics** — emitted with the global `metrics::counter!` /
  `gauge!` / `histogram!` macros. No handle to thread through; the recorder is
  global once `init_metrics` has run.

The endpoint is **pull-based**: process collectors refresh and recorder upkeep
runs inside `MetricsHandle::render()`, on each scrape — no background task, no
polling loop.

### Label anonymization (EU/GDPR — a security property)

Every label is from a **small, closed, bounded set**. No PII or user-controlled
value ever becomes a label: no user/account/tenant id, email, IP, session,
token; **no raw request path, no query string, no header or body content**. The
HTTP layer labels a request by **method + matched-route template** (the
completed-request counter and latency histogram also carry **status**; the
in-flight gauge cannot — the status is unknown until the response is produced) —
the template is Axum's `MatchedPath` (`/users/{id}`), never the concrete path
(`/users/123`). When there is no matched route the label is the
constant sentinel `"<unmatched>"` — it **fails closed**, never falling back to
the raw path. The exposition exposes only aggregates (counters / histograms /
gauges), never per-request rows. This is proved by a dedicated test.

## Why

| Thing | Why it is the way it is |
|---|---|
| HTTP route label is the `MatchedPath` template, with a `"<unmatched>"` sentinel fallback | EU/GDPR: a label must be a bounded, non-PII value. The concrete path (`/users/123`) is unbounded and user-controlled, so it is never used; an unmatched request **fails closed** to one constant, never to the raw path. Aggregates only, no per-request rows. |
| `http_requests_in_flight` is decremented on `Drop` (RAII guard), not after the await | The decrement must fire on **every** exit path. An error (`?`), a panic, or a cancelled/dropped request future never reaches the post-await code, so a manual decrement there leaks the gauge upward forever; Drop makes it impossible to skip. The counter + latency histogram stay post-await on purpose — they count **completed** requests. |
| `init_metrics` buckets only its own `http_request_duration_seconds` (`set_buckets_for_metric`), not the recorder default | Mechanism-not-policy: a global `set_buckets` would force the lib's HTTP buckets (5ms–10s) on every domain histogram the service later registers — wrong for a 30s LLM latency or a byte-size distribution. The lib pins buckets for **its own** metric only; the service sets its own per metric via `set_buckets_for_metric`. |
| Process collectors + recorder upkeep run inside `render()`, on each scrape | Pull-based by design — the scrape *is* the refresh trigger, so there is no background task and nothing polls. Aligns with the platform's "never poll" stance and mono-pod simplicity. |
| `metrics-exporter-prometheus` is pulled with `default-features = false` | Its default `http-listener` feature spins up a second `hyper` HTTP server on `:9000`. We serve `/metrics` from the service's own Axum router via `PrometheusHandle::render()`, so the listener is dead weight; `install_recorder` and `set_buckets` are not feature-gated. |
| `init_metrics` is `Result`, not infallible like `init_logging` | The Prometheus recorder installs **once** per process; a real double-install is an error worth surfacing (`MetricsError::Install`), not a silently-swallowed no-op. Logging's global subscriber tolerates a second call because a test harness legitimately re-inits; the recorder does not. |

## Usage

```rust
use axum::Router;
use br_util_axum_readiness::{ReadinessHandle, readiness_route};
use br_util_observability::{
    http_metrics_layer, init_logging, init_metrics, liveness_route, metrics_route,
};

// First thing in main: structured JSON logging.
init_logging("svc-notifier");

// Process-global Prometheus recorder + universal collectors.
let metrics = init_metrics("svc-notifier").expect("recorder installs once");

let readiness = ReadinessHandle::not_ready("starting up");
let app = Router::new()
    .route("/livez", liveness_route())                       // this crate
    .route("/readyz", readiness_route(readiness.clone()))    // br-util-axum-readiness
    .route("/metrics", metrics_route(metrics))               // this crate
    .layer(http_metrics_layer());                            // method + route template + status
```

## Tier & dependencies

Tier `util`: a technical wrapper over `tracing` / `tracing-subscriber`, `axum` /
`tower`, and the `metrics` facade (`metrics-exporter-prometheus` with the HTTP
listener disabled, `metrics-process` for the universal collectors). No domain,
no policy. Unified workspace versioning, distributed by git tag.

## Install

```toml
[dependencies]
br-util-observability = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-observability", tag = "v1.0.2" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](../../CHANGELOG.md) · [botresources.ai](https://botresources.ai)
