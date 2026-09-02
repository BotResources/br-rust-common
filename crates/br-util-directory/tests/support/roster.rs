use std::collections::BTreeMap;

use br_core_directory::{
    DIRECTORY_META_VERSION, DirectoryMeta, PublishedEntity, PublishedGroup,
    PublishedServiceAccount, PublishedUser,
};
use br_util_directory::DirectoryPublisher;
use br_util_nats_fabric::Fabric;
use sqlx::PgPool;
use uuid::Uuid;

use super::reads::{user_row, wait_for};

pub fn manifest() -> DirectoryMeta {
    DirectoryMeta {
        version: DIRECTORY_META_VERSION,
        entities: vec![PublishedEntity::Users, PublishedEntity::Groups],
    }
}

pub fn manifest_with_service_accounts() -> DirectoryMeta {
    DirectoryMeta {
        version: DIRECTORY_META_VERSION,
        entities: vec![
            PublishedEntity::Users,
            PublishedEntity::Groups,
            PublishedEntity::ServiceAccounts,
        ],
    }
}

pub fn user(email: &str) -> PublishedUser {
    PublishedUser::new(
        email.to_string(),
        Some("Ada".to_string()),
        Some("Lovelace".to_string()),
        BTreeMap::new(),
    )
    .expect("a published user")
}

pub fn nameless_user(email: &str) -> PublishedUser {
    PublishedUser::new(email.to_string(), None, None, BTreeMap::new())
        .expect("a published user with no name")
}

pub fn group(name: &str, members: &[Uuid]) -> PublishedGroup {
    PublishedGroup::new(name.to_string(), members.to_vec(), BTreeMap::new())
        .expect("a published group")
}

pub fn service_account(name: &str) -> PublishedServiceAccount {
    PublishedServiceAccount::new(name.to_string(), BTreeMap::new())
        .expect("a published service account")
}

pub async fn publish_until_projected(
    publisher: &DirectoryPublisher,
    pool: &PgPool,
    user_id: Uuid,
    value: &PublishedUser,
) {
    publish_until(publisher, user_id, value, async || {
        user_row(pool, user_id).await.is_some()
    })
    .await;
}

pub async fn publish_until(
    publisher: &DirectoryPublisher,
    user_id: Uuid,
    value: &PublishedUser,
    mut done: impl AsyncFnMut() -> bool,
) {
    wait_for(
        "the live watch to react to a put for the published user",
        async || {
            publisher
                .publish_user(user_id, value)
                .await
                .expect("publish the user");
            done().await
        },
    )
    .await;
}

pub async fn drop_published_user(fabric: &Fabric, user_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    publisher
        .retract_user(user_id)
        .await
        .expect("retract the published user");
}

pub async fn drop_published_group(fabric: &Fabric, group_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    publisher
        .retract_group(group_id)
        .await
        .expect("retract the published group");
}

pub async fn drop_published_service_account(fabric: &Fabric, service_account_id: Uuid) {
    let publisher = DirectoryPublisher::open(fabric).await.expect("publisher");
    publisher
        .retract_service_account(service_account_id)
        .await
        .expect("retract the published service account");
}
