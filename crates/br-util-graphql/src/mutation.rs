use async_graphql::SimpleObject;

#[derive(SimpleObject, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationResult {
    pub success: bool,
}

impl MutationResult {
    pub fn ok() -> Self {
        Self { success: true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ack_is_success_only() {
        assert!(MutationResult::ok().success);
    }
}
