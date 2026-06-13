use std::collections::BTreeMap;

use async_graphql::{Json, SimpleObject};

#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Affordance {
    pub action: String,
    pub allowed: bool,
    pub reason_code: Option<String>,
    pub params: Option<Json<BTreeMap<String, String>>>,
}

impl Affordance {
    pub fn allow(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            allowed: true,
            reason_code: None,
            params: None,
        }
    }

    pub fn block(action: impl Into<String>, reason_code: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            allowed: false,
            reason_code: Some(reason_code.into()),
            params: None,
        }
    }

    #[must_use]
    pub fn with_param(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.params
            .get_or_insert_with(|| Json(BTreeMap::new()))
            .0
            .insert(key.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_params(
        mut self,
        params: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let map = self.params.get_or_insert_with(|| Json(BTreeMap::new()));
        for (key, value) in params {
            map.0.insert(key.into(), value.into());
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allow_has_no_reason() {
        let a = Affordance::allow("rename");
        assert_eq!(a.action, "rename");
        assert!(a.allowed);
        assert_eq!(a.reason_code, None);
    }

    #[test]
    fn block_carries_its_reason_code() {
        let a = Affordance::block("rename", "title_generating");
        assert!(!a.allowed);
        assert_eq!(a.reason_code.as_deref(), Some("title_generating"));
    }

    #[test]
    fn reason_code_in_examples_is_a_token_not_a_sentence() {
        let a = Affordance::block("delete", "system_group_protected");
        let reason = a.reason_code.unwrap();
        assert!(
            !reason.contains(' '),
            "reason looks like a sentence: {reason}"
        );
    }

    #[test]
    fn allow_and_block_leave_params_unset() {
        assert_eq!(Affordance::allow("rename").params, None);
        assert_eq!(Affordance::block("delete", "locked").params, None);
    }

    #[test]
    fn with_param_attaches_and_is_carried() {
        let a =
            Affordance::block("retry", "cooldown_active").with_param("retry_after_seconds", "30");
        assert_eq!(
            a.params
                .as_ref()
                .and_then(|p| p.0.get("retry_after_seconds")),
            Some(&"30".to_owned())
        );
    }

    #[test]
    fn with_params_attaches_many() {
        let a = Affordance::block("invite", "seat_limit_reached")
            .with_params([("limit", "50"), ("used", "50")]);
        let params = a.params.as_ref().expect("params present");
        assert_eq!(params.0.get("limit"), Some(&"50".to_owned()));
        assert_eq!(params.0.get("used"), Some(&"50".to_owned()));
    }

    #[test]
    fn param_values_in_examples_are_tokens_not_sentences() {
        let a = Affordance::block("delete", "system_group_protected")
            .with_param("group_id", "0190f2a1")
            .with_param("min_members", "1");
        for (key, value) in &a.params.as_ref().expect("params present").0 {
            assert!(!key.contains(' '), "param key looks like a sentence: {key}");
            assert!(
                !value.contains(' '),
                "param value looks like a sentence: {value}"
            );
        }
    }
}
