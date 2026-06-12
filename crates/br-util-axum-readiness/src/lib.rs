use std::sync::{Arc, RwLock};

use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{MethodRouter, get};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Readiness {
    Ready,
    NotReady { reason: String },
}

impl Readiness {
    pub fn is_ready(&self) -> bool {
        matches!(self, Readiness::Ready)
    }
}

#[derive(Clone)]
pub struct ReadinessHandle {
    state: Arc<RwLock<Readiness>>,
}

impl ReadinessHandle {
    pub fn ready() -> Self {
        Self {
            state: Arc::new(RwLock::new(Readiness::Ready)),
        }
    }

    pub fn not_ready(reason: impl Into<String>) -> Self {
        Self {
            state: Arc::new(RwLock::new(Readiness::NotReady {
                reason: reason.into(),
            })),
        }
    }

    pub fn set_ready(&self) {
        let was_not_ready = {
            let mut guard = self.state.write().unwrap_or_else(|e| e.into_inner());
            let was_not_ready = !guard.is_ready();
            *guard = Readiness::Ready;
            was_not_ready
        };
        if was_not_ready {
            tracing::info!("readiness: UP");
        }
    }

    pub fn set_not_ready(&self, reason: impl Into<String>) {
        let reason = reason.into();
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

    pub fn snapshot(&self) -> Readiness {
        self.state.read().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn is_ready(&self) -> bool {
        self.snapshot().is_ready()
    }
}

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

        assert!(handle.is_ready());
        assert_eq!(handle.snapshot(), Readiness::Ready);

        let (status, body) = get_readyz(router(handle.clone())).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body, "ready");

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
