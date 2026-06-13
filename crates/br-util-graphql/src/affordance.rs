use async_graphql::SimpleObject;

#[derive(SimpleObject, Debug, Clone, PartialEq, Eq)]
pub struct Affordance {
    pub action: String,
    pub allowed: bool,
    pub reason_code: Option<String>,
}

impl Affordance {
    pub fn allow(action: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            allowed: true,
            reason_code: None,
        }
    }

    pub fn block(action: impl Into<String>, reason_code: impl Into<String>) -> Self {
        Self {
            action: action.into(),
            allowed: false,
            reason_code: Some(reason_code.into()),
        }
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
    fn reason_code_is_a_key_not_prose() {
        let a = Affordance::block("delete", "system_group_protected");
        let reason = a.reason_code.unwrap();
        assert!(
            !reason.contains(' '),
            "reason looks like a sentence: {reason}"
        );
    }
}
