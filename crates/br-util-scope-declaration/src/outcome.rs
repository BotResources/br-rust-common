use br_core_scope::ServiceScopesRejected;

#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum ScopeDeclarationOutcome {
    Accepted,
    Rejected(ServiceScopesRejected),
    Disabled,
}

impl ScopeDeclarationOutcome {
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Accepted | Self::Disabled)
    }
}
