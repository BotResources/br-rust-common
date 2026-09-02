use std::sync::Arc;

use br_core_directory::{DirectoryMeta, PublishedGroup, PublishedServiceAccount, PublishedUser};
use br_util_nats_fabric::{Fabric, PublishedLanguageConsumer, WatchHealth, WatchHealthReceiver};
use sqlx::PgPool;

use crate::consumer::config::DirectoryConsumerConfig;
use crate::consumer::health::{DirectoryStream, ProjectorHealth};
use crate::consumer::manifest::{ManifestState, read_manifest};
use crate::consumer::progress::{ProgressChannel, ProjectorProgressReceiver};
use crate::consumer::sink::{GroupSink, ServiceAccountSink, SinkContext, UserSink};
use crate::error::DirectoryError;
use crate::impact::ImpactStager;
use crate::keys::{groups_prefix, service_accounts_prefix, users_prefix};

pub struct DirectoryProjector {
    fabric: Fabric,
    pool: PgPool,
    config: DirectoryConsumerConfig,
    stager: Option<Arc<dyn ImpactStager>>,
    progress: ProgressChannel,
    health: ProjectorHealth,
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
            stager: None,
            progress: ProgressChannel::new(),
            health: ProjectorHealth::new(),
        }
    }

    pub fn with_impact_stager(mut self, stager: Arc<dyn ImpactStager>) -> Self {
        self.stager = Some(stager);
        self
    }

    pub fn progress(&self) -> ProjectorProgressReceiver {
        self.progress.receiver()
    }

    pub fn health(&self) -> WatchHealthReceiver {
        self.health.receiver()
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

        let _active = self
            .health
            .activate(&active_streams(watch_groups, watch_service_accounts));
        self.watch_streams(watch_groups, watch_service_accounts)
            .await
    }

    async fn watch_streams(
        &self,
        watch_groups: bool,
        watch_service_accounts: bool,
    ) -> Result<(), DirectoryError> {
        let users_watch = async {
            let users = self.user_consumer().await?;
            self.health
                .set(DirectoryStream::Users, WatchHealth::Healthy);
            let outcome = users.watch().await;
            self.health
                .set(DirectoryStream::Users, WatchHealth::Degraded);
            outcome.map_err(DirectoryError::from)
        };

        let groups_watch = async {
            if watch_groups {
                let groups = self.group_consumer().await?;
                self.health
                    .set(DirectoryStream::Groups, WatchHealth::Healthy);
                let outcome = groups.watch().await;
                self.health
                    .set(DirectoryStream::Groups, WatchHealth::Degraded);
                outcome?;
            }
            Ok::<(), DirectoryError>(())
        };

        let service_accounts_watch = async {
            if watch_service_accounts {
                let service_accounts = self.service_account_consumer().await?;
                self.health
                    .set(DirectoryStream::ServiceAccounts, WatchHealth::Healthy);
                let outcome = service_accounts.watch().await;
                self.health
                    .set(DirectoryStream::ServiceAccounts, WatchHealth::Degraded);
                outcome?;
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

    fn sink_context(&self) -> SinkContext {
        SinkContext::new(
            self.pool.clone(),
            self.stager.clone(),
            self.progress.clone(),
        )
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
        let sink = UserSink::new(self.sink_context(), self.config.clone());
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
        let sink = GroupSink::new(self.sink_context());
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
        let sink = ServiceAccountSink::new(self.sink_context());
        Ok(PublishedLanguageConsumer::open(
            &self.fabric,
            vec![service_accounts_prefix()],
            keep_all_service_account as fn(&PublishedServiceAccount) -> bool,
            sink,
        )
        .await?)
    }
}

fn active_streams(watch_groups: bool, watch_service_accounts: bool) -> Vec<DirectoryStream> {
    let mut streams = vec![DirectoryStream::Users];
    if watch_groups {
        streams.push(DirectoryStream::Groups);
    }
    if watch_service_accounts {
        streams.push(DirectoryStream::ServiceAccounts);
    }
    streams
}

fn keep_all_group(_group: &PublishedGroup) -> bool {
    true
}

fn keep_all_service_account(_service_account: &PublishedServiceAccount) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn users_are_always_an_active_stream() {
        assert_eq!(active_streams(false, false), vec![DirectoryStream::Users]);
    }

    #[test]
    fn groups_and_service_accounts_are_active_only_when_watched() {
        assert_eq!(
            active_streams(true, true),
            vec![
                DirectoryStream::Users,
                DirectoryStream::Groups,
                DirectoryStream::ServiceAccounts
            ]
        );
        assert_eq!(
            active_streams(false, true),
            vec![DirectoryStream::Users, DirectoryStream::ServiceAccounts]
        );
    }
}
