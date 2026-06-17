use std::collections::BTreeMap;

use br_core_directory::{DirectoryMeta, PublishedGroup, PublishedServiceAccount, PublishedUser};
use br_util_nats_fabric::{Fabric, KvKey, PublishedLanguagePublisher};
use uuid::Uuid;

use crate::error::DirectoryError;
use crate::keys::{
    group_key, groups_prefix, meta_key, service_account_key, service_accounts_prefix, user_key,
    users_prefix,
};
use crate::publisher::source::DirectorySource;

pub struct DirectoryPublisher {
    users: PublishedLanguagePublisher<PublishedUser>,
    groups: PublishedLanguagePublisher<PublishedGroup>,
    service_accounts: PublishedLanguagePublisher<PublishedServiceAccount>,
    meta: PublishedLanguagePublisher<DirectoryMeta>,
}

impl DirectoryPublisher {
    pub async fn open(fabric: &Fabric) -> Result<Self, DirectoryError> {
        Ok(Self {
            users: PublishedLanguagePublisher::open(fabric).await?,
            groups: PublishedLanguagePublisher::open(fabric).await?,
            service_accounts: PublishedLanguagePublisher::open(fabric).await?,
            meta: PublishedLanguagePublisher::open(fabric).await?,
        })
    }

    pub async fn reconcile<S: DirectorySource>(&self, source: &S) -> Result<(), DirectoryError> {
        let manifest = source.manifest();

        let desired_users = if manifest.publishes_users() {
            keyed(source.desired_users().await?, user_key)?
        } else {
            BTreeMap::new()
        };
        self.users
            .reconcile(&users_prefix(), &desired_users)
            .await?;

        let desired_groups = if manifest.publishes_groups() {
            keyed(source.desired_groups().await?, group_key)?
        } else {
            BTreeMap::new()
        };
        self.groups
            .reconcile(&groups_prefix(), &desired_groups)
            .await?;

        let desired_service_accounts = if manifest.publishes_service_accounts() {
            keyed(
                source.desired_service_accounts().await?,
                service_account_key,
            )?
        } else {
            BTreeMap::new()
        };
        self.service_accounts
            .reconcile(&service_accounts_prefix(), &desired_service_accounts)
            .await?;

        self.write_meta(&manifest).await
    }

    pub async fn publish_user(
        &self,
        user_id: Uuid,
        user: &PublishedUser,
    ) -> Result<(), DirectoryError> {
        self.users.put(&user_key(user_id)?, user).await?;
        Ok(())
    }

    pub async fn retract_user(&self, user_id: Uuid) -> Result<(), DirectoryError> {
        self.users.retract(&user_key(user_id)?).await?;
        Ok(())
    }

    pub async fn publish_group(
        &self,
        group_id: Uuid,
        group: &PublishedGroup,
    ) -> Result<(), DirectoryError> {
        self.groups.put(&group_key(group_id)?, group).await?;
        Ok(())
    }

    pub async fn retract_group(&self, group_id: Uuid) -> Result<(), DirectoryError> {
        self.groups.retract(&group_key(group_id)?).await?;
        Ok(())
    }

    pub async fn publish_service_account(
        &self,
        service_account_id: Uuid,
        service_account: &PublishedServiceAccount,
    ) -> Result<(), DirectoryError> {
        self.service_accounts
            .put(&service_account_key(service_account_id)?, service_account)
            .await?;
        Ok(())
    }

    pub async fn retract_service_account(
        &self,
        service_account_id: Uuid,
    ) -> Result<(), DirectoryError> {
        self.service_accounts
            .retract(&service_account_key(service_account_id)?)
            .await?;
        Ok(())
    }

    pub async fn write_meta(&self, manifest: &DirectoryMeta) -> Result<(), DirectoryError> {
        self.meta.put(&meta_key(), manifest).await?;
        Ok(())
    }
}

fn keyed<V>(
    by_id: BTreeMap<Uuid, V>,
    key_for: fn(Uuid) -> Result<KvKey, br_util_nats_fabric::KvKeyError>,
) -> Result<BTreeMap<KvKey, V>, DirectoryError> {
    by_id
        .into_iter()
        .map(|(id, value)| Ok((key_for(id)?, value)))
        .collect()
}
