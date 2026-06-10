//! A readiness gate for HTTP services — a cloneable toggle plus an Axum handler.
//!
//! **Readiness, not liveness.** This gate reports whether the service should
//! receive traffic *right now*; it never signals that the process is dead. A
//! service that reports not-ready is taken **out of rotation** (Kubernetes
//! routes no new requests to it) but is **not restarted** — a restart is driven
//! by a failed *liveness* probe, which this crate deliberately does not provide.
//! That distinction is the whole point: a service that fails a startup check
//! stays alive and inspectable instead of crash-looping.
//!
//! What gates readiness is the **caller's** concern. Hold a [`ReadinessHandle`],
//! start it [`not_ready`] with a reason, and flip it to [`ready`] once your
//! startup work succeeds — a dependency becomes reachable, a cache warms, a boot
//! handshake is confirmed. This crate only carries the up/down state and serves
//! it on `/readyz` via [`readiness_route`]; it knows nothing about *why* a given
//! service is or isn't ready.
//!
//! [`not_ready`]: ReadinessHandle::not_ready
//! [`ready`]: ReadinessHandle::ready

use std::sync::{Arc, RwLock};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

/// The current readiness of a service.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    /// The service should receive traffic.
    Ready,
    /// The service must be taken out of rotation. `reason` is operator-facing
    /// copy, surfaced verbatim in the `/readyz` response body and in logs —
    /// never put a secret or sensitive internal detail in it.
    NotReady { reason: String },
}

impl Readiness {
    /// Whether this state is [`Readiness::Ready`].
    pub fn is_ready(&self) -> bool {
        matches!(self, Readiness::Ready)
    }
}

/// A cloneable handle to a service's readiness state.
///
/// Clone it freely: every clone shares the same underlying state, so your
/// startup logic and the Axum handler built by [`readiness_route`] always
/// observe the same value.
#[derive(Clone)]
pub struct ReadinessHandle {
    state: Arc<RwLock<Readiness>>,
}

impl ReadinessHandle {
    /// A handle that starts **ready**. Use for services that never gate but
    /// still want to expose a `/readyz` probe.
    pub fn ready() -> Self {
        Self {
            state: Arc::new(RwLock::new(Readiness::Ready)),
        }
    }

    /// A handle that starts **not ready** with `reason`. The safe default for a
    /// gating service: it serves no traffic until something flips it to ready.
    pub fn not_ready(reason: impl Into<String>) -> Self {
        Self {
            state: Arc::new(RwLock::new(Readiness::NotReady {
                reason: reason.into(),
            })),
        }
    }

    /// Flip to **ready**. Idempotent; logs only on an actual transition.
    pub fn set_ready(&self) {
        // Decide and mutate under the lock; log *after* releasing it — never
        // hold the write lock across `tracing` I/O (it would serialize every
        // `/readyz` reader behind it).
        let was_not_ready = {
            // Recover from a poisoned lock rather than propagate the panic.
            // See [`Self::snapshot`] for why this is safe and necessary: every
            // mutation here is a single infallible assignment with no I/O, so a
            // poisoned guard cannot expose a torn `Readiness`, and the readiness
            // gate must keep answering even if some unrelated writer panicked.
            let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
            let was_not_ready = !guard.is_ready();
            *guard = Readiness::Ready;
            was_not_ready
        };
        if was_not_ready {
            tracing::info!("readiness: UP");
        }
    }

    /// Flip to **not ready** with `reason`. Idempotent; logs on a transition
    /// into not-ready or when the reason changes (so it never spams the log).
    pub fn set_not_ready(&self, reason: impl Into<String>) {
        let reason = reason.into();
        // See `set_ready`: mutate under the lock, log outside it, and recover
        // a poisoned lock instead of propagating the panic.
        let changed = {
            let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
            let changed = match &*guard {
                Readiness::Ready => true,
                Readiness::NotReady { reason: prev } => prev != &reason,
            };
            *guard = Readiness::NotReady {
                reason: reason.clone(),
            };
            changed
        };
        if changed {
            tracing::warn!(reason = %reason, "readiness: DOWN");
        }
    }

    /// A snapshot of the current readiness.
    ///
    /// **Poison recovery.** A poisoned lock (some thread panicked while holding
    /// the write guard) is recovered into its inner value rather than
    /// re-panicking. This is deliberate and is the safe choice *for this type*:
    /// `Readiness` is a plain enum and every write here is a single infallible
    /// assignment with no I/O, so a panic can never leave the state half-written
    /// — there is no torn value to protect against. The opposite (propagating
    /// the poison) would make a *reader*, the `/readyz` probe, panic and 500
    /// because some unrelated writer once panicked; a readiness gate's own
    /// failure mode must fail closed (report a real up/down state, taken out of
    /// rotation if down), never abort the probe. So the gate keeps answering.
    pub fn snapshot(&self) -> Readiness {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Whether the service is currently ready.
    pub fn is_ready(&self) -> bool {
        self.snapshot().is_ready()
    }
}

/// An Axum `GET` route reporting the readiness behind `handle`.
///
/// `200 OK` (body `"ready"`) when ready, `503 Service Unavailable` (body = the
/// not-ready reason) otherwise. It is generic over the router state, so it
/// mounts into any `Router<S>`. Use the conventional `/readyz` path:
///
/// ```
/// use axum::Router;
/// use br_util_axum_readiness::{ReadinessHandle, readiness_route};
///
/// let readiness = ReadinessHandle::not_ready("starting up");
/// let app: Router = Router::new().route("/readyz", readiness_route(readiness.clone()));
/// // ... hand `readiness` to the component that flips it once ready.
/// ```
pub fn readiness_route<S>(handle: ReadinessHandle) -> MethodRouter<S>
where
    S: Clone + Send + Sync + 'static,
{
    get(move || {
        let handle = handle.clone();
        async move {
            match handle.snapshot() {
                Readiness::Ready => (StatusCode::OK, "ready").into_response(),
                Readiness::NotReady { reason } => {
                    (StatusCode::SERVICE_UNAVAILABLE, reason).into_response()
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn ready_handle_is_ready() {
        let h = ReadinessHandle::ready();
        assert!(h.is_ready());
        assert_eq!(h.snapshot(), Readiness::Ready);
    }

    #[test]
    fn not_ready_handle_carries_its_reason() {
        let h = ReadinessHandle::not_ready("starting up");
        assert!(!h.is_ready());
        assert_eq!(
            h.snapshot(),
            Readiness::NotReady {
                reason: "starting up".to_string()
            }
        );
    }

    #[test]
    fn clones_share_one_state() {
        let h = ReadinessHandle::not_ready("starting up");
        let h2 = h.clone();

        h.set_ready();
        assert!(h2.is_ready(), "a flip on one clone is visible on another");

        h2.set_not_ready("dependency unavailable");
        assert_eq!(
            h.snapshot(),
            Readiness::NotReady {
                reason: "dependency unavailable".to_string()
            },
            "and back again, from the other clone"
        );
    }

    fn router(handle: ReadinessHandle) -> Router {
        Router::new().route("/readyz", readiness_route(handle))
    }

    async fn get_readyz(app: Router) -> (StatusCode, String) {
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(bytes.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn readyz_returns_200_ready_when_ready() {
        let (status, body) = get_readyz(router(ReadinessHandle::ready())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ready");
    }

    #[tokio::test]
    async fn readyz_returns_503_with_reason_when_not_ready() {
        let (status, body) =
            get_readyz(router(ReadinessHandle::not_ready("dependency unavailable"))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, "dependency unavailable");
    }

    #[tokio::test]
    async fn readyz_still_answers_after_the_lock_is_poisoned() {
        // Poison the lock by panicking a thread while it holds the write guard,
        // then assert the probe still answers (recovered, not 500/aborted) and
        // reports the last-written state. A reader panicking because some
        // unrelated writer panicked is exactly the failure this gate must avoid.
        let handle = ReadinessHandle::not_ready("starting up");
        handle.set_ready();

        let poison_target = handle.clone();
        let poisoned = std::thread::spawn(move || {
            let _guard = poison_target.state.write().unwrap();
            panic!("poison the readiness lock mid-mutation");
        })
        .join();
        assert!(poisoned.is_err(), "the spawned thread must have panicked");
        assert!(
            handle.state.is_poisoned(),
            "the lock must now be poisoned for this test to mean anything"
        );

        // Direct reads recover instead of panicking.
        assert!(handle.is_ready());
        assert_eq!(handle.snapshot(), Readiness::Ready);

        // And the HTTP probe answers normally rather than 500-ing.
        let (status, body) = get_readyz(router(handle.clone())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ready");

        // A write path also recovers and keeps working.
        handle.set_not_ready("dependency unavailable");
        let (status, body) = get_readyz(router(handle)).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body, "dependency unavailable");
    }

    #[tokio::test]
    async fn readyz_reflects_a_runtime_flip() {
        let handle = ReadinessHandle::not_ready("starting up");
        let app = router(handle.clone());

        let (status, _) = get_readyz(app.clone()).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

        handle.set_ready();
        let (status, body) = get_readyz(app).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ready");
    }
}
