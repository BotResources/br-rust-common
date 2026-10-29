#![doc = include_str!("../README.md")]

mod declaration;
mod error;
mod implication;
mod key;
mod messages;
mod raw;
mod service;
mod spec;

pub use declaration::{ScopeDeclaration, ScopeDeclarationV2};
pub use error::{KeyValidationError, ScopeDeclarationError};
pub use key::{SCOPE_KEY_MAX_LEN, ScopeKey};
pub use messages::{
    DeclareServiceScopes, DeclareServiceScopesV2, ServiceScopesAccepted, ServiceScopesRejected,
};
pub use raw::{
    RawScopeDeclaration, RawScopeDeclarationV2, RawScopeSpec, RawScopeSpecV2, RawServiceManifest,
};
pub use service::{SERVICE_KEY_MAX_LEN, ServiceKey};
pub use spec::{ScopeSpec, ScopeSpecV2, ServiceManifest};
