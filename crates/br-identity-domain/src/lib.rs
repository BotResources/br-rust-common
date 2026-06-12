#![doc = include_str!("../README.md")]

mod error;
mod event;
mod handler;
mod registry;
mod service;

pub use br_core_scope::{ScopeDeclaration, ScopeDeclarationError};

pub use error::RegistryHydrationError;
pub use event::{CommandResult, RegistryEvent, RegistryWarning};
pub use handler::{DeclarationOutcome, judge_declaration};
pub use registry::ScopeRegistry;
pub use service::RegisteredService;
