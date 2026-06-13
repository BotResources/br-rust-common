#![doc = include_str!("../README.md")]

mod conflict;
mod consumer;
mod error;
mod hydration;
mod migrations;
mod pipeline;
mod publisher;
mod repository;

pub use conflict::SaveOutcome;
pub use consumer::run_scope_declarations;
pub use error::AppError;
pub use migrations::migrate;
pub use pipeline::ScopeDeclarationPipeline;
pub use publisher::ConfirmationPublisher;
pub use repository::ScopeRegistryRepository;
