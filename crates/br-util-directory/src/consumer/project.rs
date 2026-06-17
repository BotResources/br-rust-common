use br_core_directory::{DirectoryMeta, PublishedGroup, PublishedServiceAccount, PublishedUser};
use br_util_nats_fabric::{Fabric, PublishedLanguageConsumer};
use sqlx::PgPool;

use crate::consumer::config::DirectoryConsumerConfig;
use crate::consumer::manifest::{ManifestState, read_manifest};
use crate::consumer::sink::{GroupSink, ServiceAccountSink, UserSink};
use crate::error::DirectoryError;
use crate::keys::{groups_prefix, service_accounts_prefix, users_prefix};

pub struct DirectoryProjector {
    fabric: Fabric,
    pool: PgPool,
    config: DirectoryConsumerConfig,
}

impl DirectoryProjector {
    pub fn new(fabric: Fabric, pool: PgPool) -> Self {
        Self::with_config(fabric, pool, DirectoryConsumerConfig::default())
    }

    pub fn with_config(fabric: Fabric, pool: PgPool, config: DirectoryConsumerConfig) -> Self {
        Self {
            fabric,
            pool,
            config,
        }
    }

    pub async fn reconcile(&self) -> Result<DirectoryMeta, DirectoryError> {
        let manifest = self.present_manifest().await?;

        self.user_consumer().await?.bootstrap().await?;

        if self.config.consumption_scope().consumes_groups() {
            self.group_consumer().await?.bootstrap().await?;
        }

        if manifest.publishes_service_accounts() {
            self.service_account_consumer().await?.bootstrap().await?;
        }

        Ok(manifest)
    }

    pub async fn watch(&self) -> Result<(), DirectoryError> {
        let manifest = self.present_manifest().await?;
        let watch_groups = self.config.consumption_scope().consumes_groups();
        let watch_service_accounts = manifest.publishes_service_accounts();

        let users = self.user_consumer().await?;
        let users_watch = async { users.watch().await.map_err(DirectoryError::from) };

        let groups_watch = async {
            if watch_groups {
                self.group_consumer().await?.watch().await?;
            }
            Ok::<(), DirectoryError>(())
        };

        let service_accounts_watch = async {
            if watch_service_accounts {
                self.service_account_consumer().await?.watch().await?;
            }
            Ok::<(), DirectoryError>(())
        };

        tokio::try_join!(users_watch, groups_watch, service_accounts_watch)?;
        Ok(())
    }

    async fn present_manifest(&self) -> Result<DirectoryMeta, DirectoryError> {
        match read_manifest(&self.fabric).await? {
            ManifestState::Present(meta) => Ok(meta),
            ManifestState::Absent => Err(DirectoryError::ManifestAbsent),
        }
    }

    async fn user_consumer(
        &self,
    ) -> Result<
        PublishedLanguageConsumer<
            PublishedUser,
            impl Fn(&PublishedUser) -> bool + Send + Sync,
            UserSink,
        >,
        DirectoryError,
    > {
        let filter = self.config.user_copy_filter();
        let sink = UserSink::new(self.pool.clone(), self.config.clone());
        Ok(PublishedLanguageConsumer::open(
            &self.fabric,
            vec![users_prefix()],
            move |user: &PublishedUser| (filter)(user),
            sink,
        )
        .await?)
    }

    async fn group_consumer(
        &self,
    ) -> Result<
        PublishedLanguageConsumer<PublishedGroup, fn(&PublishedGroup) -> bool, GroupSink>,
        DirectoryError,
    > {
        let sink = GroupSink::new(self.pool.clone());
        Ok(PublishedLanguageConsumer::open(
            &self.fabric,
            vec![groups_prefix()],
            keep_all_group as fn(&PublishedGroup) -> bool,
            sink,
        )
        .await?)
    }

    async fn service_account_consumer(
        &self,
    ) -> Result<
        PublishedLanguageConsumer<
            PublishedServiceAccount,
            fn(&PublishedServiceAccount) -> bool,
            ServiceAccountSink,
        >,
        DirectoryError,
    > {
        let sink = ServiceAccountSink::new(self.pool.clone());
        Ok(PublishedLanguageConsumer::open(
            &self.fabric,
            vec![service_accounts_prefix()],
            keep_all_service_account as fn(&PublishedServiceAccount) -> bool,
            sink,
        )
        .await?)
    }
}

fn keep_all_group(_group: &PublishedGroup) -> bool {
    true
}

fn keep_all_service_account(_service_account: &PublishedServiceAccount) -> bool {
    true
}
