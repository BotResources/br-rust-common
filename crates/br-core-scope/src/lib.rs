#![doc = include_str!("../README.md")]

mod declaration;
mod error;
mod key;
mod messages;
mod raw;
mod service;
mod spec;

pub use declaration::ScopeDeclaration;
pub use error::{KeyValidationError, ScopeDeclarationError};
pub use key::{SCOPE_KEY_MAX_LEN, ScopeKey};
pub use messages::{DeclareServiceScopes, ServiceScopesAccepted, ServiceScopesRejected};
pub use raw::{RawScopeDeclaration, RawScopeSpec, RawServiceManifest};
pub use service::{SERVICE_KEY_MAX_LEN, ServiceKey};
pub use spec::{ScopeSpec, ServiceManifest};
