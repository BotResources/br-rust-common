use std::collections::BTreeMap;

use br_core_directory::{DirectoryMeta, PublishedGroup, PublishedServiceAccount, PublishedUser};
use uuid::Uuid;

use crate::error::DirectoryError;

#[async_trait::async_trait]
pub trait DirectorySource: Send + Sync {
    fn manifest(&self) -> DirectoryMeta;

    async fn desired_users(&self) -> Result<BTreeMap<Uuid, PublishedUser>, DirectoryError>;

    async fn desired_groups(&self) -> Result<BTreeMap<Uuid, PublishedGroup>, DirectoryError>;

    async fn desired_service_accounts(
        &self,
    ) -> Result<BTreeMap<Uuid, PublishedServiceAccount>, DirectoryError> {
        Ok(BTreeMap::new())
    }
}
