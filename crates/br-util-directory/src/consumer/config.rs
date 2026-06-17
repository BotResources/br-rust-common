use std::sync::Arc;

use br_core_directory::PublishedUser;
use serde_json::Value;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ConsumptionScope {
    UsersOnly,
    #[default]
    UsersAndGroups,
}

impl ConsumptionScope {
    pub fn consumes_groups(self) -> bool {
        matches!(self, Self::UsersAndGroups)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistedExtensions(Value);

impl PersistedExtensions {
    pub fn none() -> Self {
        Self(Value::Object(serde_json::Map::new()))
    }

    pub fn from_value(value: Value) -> Self {
        Self(value)
    }

    pub fn into_value(self) -> Value {
        self.0
    }

    pub fn as_value(&self) -> &Value {
        &self.0
    }
}

impl Default for PersistedExtensions {
    fn default() -> Self {
        Self::none()
    }
}

type ExtractFn = Arc<dyn Fn(&PublishedUser) -> PersistedExtensions + Send + Sync>;
type FilterFn = Arc<dyn Fn(&PublishedUser) -> bool + Send + Sync>;

#[derive(Clone)]
pub struct DirectoryConsumerConfig {
    scope: ConsumptionScope,
    extract: ExtractFn,
    filter: FilterFn,
}

impl DirectoryConsumerConfig {
    pub fn scope(mut self, scope: ConsumptionScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn extract_user_extensions(
        mut self,
        extract: impl Fn(&PublishedUser) -> PersistedExtensions + Send + Sync + 'static,
    ) -> Self {
        self.extract = Arc::new(extract);
        self
    }

    pub fn filter_users(
        mut self,
        filter: impl Fn(&PublishedUser) -> bool + Send + Sync + 'static,
    ) -> Self {
        self.filter = Arc::new(filter);
        self
    }

    pub fn consumption_scope(&self) -> ConsumptionScope {
        self.scope
    }

    pub(crate) fn extract_for(&self, user: &PublishedUser) -> PersistedExtensions {
        (self.extract)(user)
    }

    pub(crate) fn user_copy_filter(&self) -> FilterFn {
        self.filter.clone()
    }
}

impl Default for DirectoryConsumerConfig {
    fn default() -> Self {
        Self {
            scope: ConsumptionScope::default(),
            extract: Arc::new(|_user| PersistedExtensions::none()),
            filter: Arc::new(|_user| true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scope_is_users_and_groups() {
        assert_eq!(
            DirectoryConsumerConfig::default().consumption_scope(),
            ConsumptionScope::UsersAndGroups
        );
        assert!(ConsumptionScope::UsersAndGroups.consumes_groups());
        assert!(!ConsumptionScope::UsersOnly.consumes_groups());
    }

    fn user_with_extension(key: &str, value: Value) -> PublishedUser {
        let mut extensions = std::collections::BTreeMap::new();
        extensions.insert(key.to_string(), value);
        PublishedUser::new("a@example.com".to_string(), None, None, extensions).unwrap()
    }

    #[test]
    fn default_extract_keeps_nothing() {
        let config = DirectoryConsumerConfig::default();
        let user = user_with_extension("locale", Value::from("fr"));
        assert_eq!(
            config.extract_for(&user),
            PersistedExtensions::none(),
            "default must persist no extension"
        );
    }

    #[test]
    fn default_filter_keeps_all() {
        let config = DirectoryConsumerConfig::default();
        let user = user_with_extension("is_platform_member", Value::from(false));
        assert!((config.user_copy_filter())(&user));
    }

    #[test]
    fn custom_filter_scopes_the_roster() {
        let config = DirectoryConsumerConfig::default()
            .filter_users(|user| user.extension("is_platform_member") == Some(&Value::from(true)));
        let member = user_with_extension("is_platform_member", Value::from(true));
        let outsider = user_with_extension("is_platform_member", Value::from(false));
        let filter = config.user_copy_filter();
        assert!((filter)(&member));
        assert!(!(filter)(&outsider));
    }

    #[test]
    fn custom_extract_selects_the_persisted_payload() {
        let config = DirectoryConsumerConfig::default().extract_user_extensions(|user| match user
            .extension("locale")
        {
            Some(value) => {
                PersistedExtensions::from_value(serde_json::json!({ "locale": value.clone() }))
            }
            None => PersistedExtensions::none(),
        });
        let user = user_with_extension("locale", Value::from("fr"));
        assert_eq!(
            config.extract_for(&user).into_value(),
            serde_json::json!({ "locale": "fr" })
        );
    }
}
