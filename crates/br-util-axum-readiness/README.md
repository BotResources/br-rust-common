# br-util-axum-readiness

A readiness gate for HTTP services: a cloneable UP/DOWN toggle plus an Axum
`/readyz` handler. Thin technical wrapper (tier `util`); it enforces no domain
policy and knows nothing about *why* a service is ready.

## Readiness, not liveness

The gate reports whether the service should receive traffic **right now**. A
not-ready service is taken **out of rotation** (Kubernetes routes no new
requests to it) but is **not restarted** — a restart is driven by a *liveness*
probe, which this crate deliberately does not provide. So a service that fails a
startup check stays alive and inspectable instead of crash-looping. Wire
`/readyz` to this gate and your *liveness* probe (if any) to a separate,
always-200 endpoint.

**What gates readiness is the caller's concern.** Start the handle `not_ready`
with a reason and flip it to `ready` once your startup work succeeds — a
dependency becomes reachable, a cache warms, a boot handshake is confirmed. This
crate carries only the state and serves it; it does not decide readiness for you.

## What's inside

| Item | Kind | Behavior |
|---|---|---|
| `ReadinessHandle` | cloneable handle | Shared UP/DOWN state with an operator-facing reason. `ready()` / `not_ready(reason)` constructors; `set_ready()` / `set_not_ready(reason)` toggles; `snapshot()` / `is_ready()` reads. Clones share one state. Transitions logged via `tracing`. |
| `Readiness` | enum | `Ready` \| `NotReady { reason }`. |
| `readiness_route::<S>` | `fn(ReadinessHandle) -> MethodRouter<S>` | Axum `GET` route: `200 OK` (body `"ready"`) when ready, `503 Service Unavailable` (body = reason) otherwise. Generic over the router state, so it mounts into any `Router<S>`. |

The `reason` is returned verbatim in the `503` body and emitted in logs, so it
is **operator-facing copy only** — never put a secret, a credential, or a
sensitive internal detail (DB URL, stack trace) in it.

## Usage

```rust
use axum::{Router, routing::get};
use br_util_axum_readiness::{ReadinessHandle, readiness_route};

// Start not-ready; the service serves no traffic until it flips.
let readiness = ReadinessHandle::not_ready("starting up");

let app = Router::new()
    .route("/readyz", readiness_route(readiness.clone()))
    .route("/livez", get(|| async { "ok" })); // liveness is separate, always 200

// Hand `readiness` to your startup logic:
//   on success -> readiness.set_ready();
//   on failure -> readiness.set_not_ready("dependency X unavailable");
```

## Install

```toml
[dependencies]
br-util-axum-readiness = { git = "https://github.com/BotResources/br-rust-common", package = "br-util-axum-readiness", tag = "br-util-axum-readiness-v0.1.0" }
```

---

Part of [`br-rust-common`](../../README.md) · [Changelog](CHANGELOG.md) · [botresources.ai](https://botresources.ai)
