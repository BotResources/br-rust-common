#![doc = include_str!("../README.md")]

pub mod actor;
mod config;
mod handshake;
mod outcome;
mod subjects;

pub use actor::declaring_actor;
pub use config::ScopeDeclarationConfig;
pub use handshake::declare_scopes;
pub use outcome::ScopeDeclarationOutcome;
