use br_core_directory::{DirectoryMeta, META_KEY};
use br_util_nats_fabric::{Fabric, KvKey, PublishedLanguageReader};

use crate::error::DirectoryError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestState {
    Present(DirectoryMeta),
    Absent,
}

pub async fn read_manifest(fabric: &Fabric) -> Result<ManifestState, DirectoryError> {
    let key = KvKey::new(META_KEY)?;
    let reader = PublishedLanguageReader::<DirectoryMeta>::open(fabric).await?;
    Ok(match reader.get(&key).await? {
        Some(meta) => ManifestState::Present(meta),
        None => ManifestState::Absent,
    })
}
