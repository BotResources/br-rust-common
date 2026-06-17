#![doc = include_str!("../README.md")]

mod error;
mod flatten;
mod group;
mod keys;
mod meta;
mod service_account;
mod user;

pub use error::DirectoryError;
pub use group::{PUBLISHED_GROUP_RESERVED_KEYS, PublishedGroup};
pub use keys::{
    GROUPS_KEY_PREFIX, META_KEY, SERVICE_ACCOUNTS_KEY_PREFIX, USERS_KEY_PREFIX,
    group_id_from_kv_key, group_kv_key, service_account_id_from_kv_key, service_account_kv_key,
    user_id_from_kv_key, user_kv_key,
};
pub use meta::{DIRECTORY_META_VERSION, DirectoryMeta, PublishedEntity};
pub use service_account::{PUBLISHED_SERVICE_ACCOUNT_RESERVED_KEYS, PublishedServiceAccount};
pub use user::{PUBLISHED_USER_RESERVED_KEYS, PublishedUser};
