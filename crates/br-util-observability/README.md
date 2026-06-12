# br-util-observability

One boot-time observability setup for every BotResources process: structured
JSON logging plus an always-200 `/livez` liveness route. Thin technical wrapper
(tier `util`); it enforces no domain policy.

## What's inside

| Item | Kind | Behavior |
|---|---|---|
| `init_logging` | `fn(component: &str)` | Installs a global `tracing` subscriber that emits **one JSON object per line** on stdout. Canonical keys: `ts` (RFC 3339, UTC), `level`, `component`, `msg`; every event field is carried alongside. Level is env-driven (`RUST_LOG`, default `info`). Idempotent — a second call is a no-op (logs a notice, never panics). Call once, first thing in `main`. |
| `liveness_route::<S>` | `fn() -> MethodRouter<S>` | Axum `GET` route, **always** `200 OK` (body `"alive"`). Generic over the router state, so it mounts into any `Router<S>`. |

## Logging

Each line is a self-contained JSON object:

```json
{"ts":"2026-06-12T10:00:00+00:00","level":"INFO","component":"composer","msg":"started","port":8080}
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

## Metrics — not in v0.1.0 (deliberate)

A Prometheus `/metrics` endpoint is **out of scope** for this version. No BR
process exposes metrics yet, and pulling a metrics client into a load-bearing
tier-`util` crate would force a shared-version coupling on every consumer for a
surface nobody uses. When a real metrics need lands it earns its own design,
not a speculative endpoint here.

## Usage

```rust
use axum::Router;
use br_util_axum_readiness::{ReadinessHandle, readiness_route};
use br_util_observability::{init_logging, liveness_route};

// First thing in main: structured JSON logging.
init_logging("svc-notifier");

let readiness = ReadinessHandle::not_ready("starting up");
let app = Router::new()
    .route("/livez", liveness_route())                       // this crate
    .route("/readyz", readiness_route(readiness.clone()));   // br-util-axum-readiness
```

## Tier & dependencies

Tier `util`: a technical wrapper over `tracing` / `tracing-subscriber` and
`axum`. No domain, no policy. Per-crate semver, distributed by git tag.

## Install

```toml
[dependencies]
br-util-observability = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-observability", tag = "br-util-observability-v0.1.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
