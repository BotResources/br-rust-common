use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use br_core_directory::{DirectoryMeta, META_KEY};
use br_util_nats_fabric::{Fabric, KvKey, KvPrefix, ProjectionSink, PublishedLanguageConsumer};

use crate::error::DirectoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestState {
    Present(DirectoryMeta),
    Absent,
}

pub async fn read_manifest(fabric: &Fabric) -> Result<ManifestState, DirectoryError> {
    let prefix = KvPrefix::new(META_KEY).expect("frozen meta key is a valid kv prefix");
    let sink = CaptureSink::default();
    let captured = sink.clone();
    let consumer =
        PublishedLanguageConsumer::open(fabric, vec![prefix], |_: &DirectoryMeta| true, sink)
            .await?;
    consumer.bootstrap().await.map_err(DirectoryError::from)?;
    Ok(match captured.take() {
        Some(meta) => ManifestState::Present(meta),
        None => ManifestState::Absent,
    })
}

#[derive(Default, Clone)]
struct CaptureSink {
    manifest: Arc<Mutex<Option<DirectoryMeta>>>,
}

impl CaptureSink {
    fn take(&self) -> Option<DirectoryMeta> {
        self.manifest
            .lock()
            .expect("manifest mutex poisoned")
            .take()
    }
}

#[async_trait::async_trait]
impl ProjectionSink<DirectoryMeta> for CaptureSink {
    type Error = DirectoryError;

    async fn project(&self, _key: &KvKey, value: &DirectoryMeta) -> Result<(), Self::Error> {
        *self.manifest.lock().expect("manifest mutex poisoned") = Some(value.clone());
        Ok(())
    }

    async fn retract(&self, _key: &KvKey) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn known_keys(&self) -> Result<BTreeSet<KvKey>, Self::Error> {
        Ok(BTreeSet::new())
    }
}
