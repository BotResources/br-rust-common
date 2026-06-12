//! One boot-time observability setup for every BotResources process.
//!
//! Tier `util` — a thin technical wrapper: types and functions, no domain, no
//! policy. It gives a process two things, both of which were otherwise hand-
//! rolled per binary:
//!
//! - [`init_logging`] — structured **JSON logging** on stdout (one object per
//!   line: `ts` / `level` / `component` / `msg` + fields), env-driven level
//!   (`RUST_LOG`, default `info`). Call it once, first thing in `main`.
//! - [`liveness_route`] — an always-`200` `/livez` Axum handler.
//!
//! ## Complementary to `br-util-axum-readiness`, never overlapping
//!
//! The two probes answer different questions and drive different orchestrator
//! actions; keeping them in separate crates keeps that split honest:
//!
//! | Probe | Crate | Question | A failure means |
//! |---|---|---|---|
//! | `/livez` | **this crate** | is the process alive? | Kubernetes **restarts** it |
//! | `/readyz` | `br-util-axum-readiness` | should it receive traffic *now*? | taken **out of rotation**, not killed |
//!
//! This crate ships **no readiness gate** — point readiness at
//! `br-util-axum-readiness`. Liveness is unconditional by design: gating it on
//! a dependency would turn a transient outage into a crash-loop.
//!
//! ## Metrics — not in v0.1.0 (deliberate)
//!
//! A Prometheus `/metrics` endpoint is **out of scope** for this version. No BR
//! process exposes metrics yet, and pulling a metrics client into a load-bearing
//! tier-`util` crate would force a shared-version coupling on every consumer for
//! a surface nobody uses. When a real metrics need lands, it earns its own
//! design rather than a speculative endpoint here.

mod health;
mod logging;
mod visitor;

pub use health::liveness_route;
pub use logging::init_logging;
