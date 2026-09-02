use serde::de::DeserializeOwned;
use tokio::sync::Mutex;

use crate::error::FabricError;
use crate::kv::ephemeral_auth_watch::{EphemeralAuthChange, EphemeralAuthWatcher};
use crate::kv::supervisor::{FollowReport, ReconcileCycle};

#[async_trait::async_trait]
pub(crate) trait ChangeSource {
    type Value;

    async fn bucket_present(&self) -> Result<(), FabricError>;

    async fn follow_changes(
        &self,
        on_change: &mut (dyn FnMut(EphemeralAuthChange<Self::Value>) + Send),
    ) -> Result<(), FabricError>;

    fn observed(&self) -> u64;
}

#[async_trait::async_trait]
impl<V> ChangeSource for EphemeralAuthWatcher<V>
where
    V: DeserializeOwned + Send + Sync,
{
    type Value = V;

    async fn bucket_present(&self) -> Result<(), FabricError> {
        self.store()
            .stream
            .get_info()
            .await
            .map_err(FabricError::kv)?;
        Ok(())
    }

    async fn follow_changes(
        &self,
        on_change: &mut (dyn FnMut(EphemeralAuthChange<V>) + Send),
    ) -> Result<(), FabricError> {
        self.watch(on_change).await
    }

    fn observed(&self) -> u64 {
        EphemeralAuthWatcher::observed(self)
    }
}

pub(crate) struct SupervisedWatch<'a, S: ?Sized, H> {
    source: &'a S,
    on_change: Mutex<H>,
}

impl<'a, S: ?Sized, H> SupervisedWatch<'a, S, H> {
    pub(crate) fn new(source: &'a S, on_change: H) -> Self {
        Self {
            source,
            on_change: Mutex::new(on_change),
        }
    }
}

#[async_trait::async_trait]
impl<S, H> ReconcileCycle for SupervisedWatch<'_, S, H>
where
    S: ChangeSource + Sync + ?Sized,
    S::Value: Send,
    H: FnMut(EphemeralAuthChange<S::Value>) + Send,
{
    async fn reconcile(&self) -> Result<(), String> {
        self.source
            .bucket_present()
            .await
            .map_err(|e| e.to_string())
    }

    async fn follow(&self) -> FollowReport {
        let mut handler = self.on_change.lock().await;
        let observed_before = self.source.observed();
        let mut delivered = false;
        let outcome = {
            let mut record = |change| {
                delivered = true;
                (*handler)(change);
            };
            self.source.follow_changes(&mut record).await
        };
        FollowReport {
            progressed: delivered || self.source.observed() > observed_before,
            outcome: outcome.map_err(|e| e.to_string()),
        }
    }
}
