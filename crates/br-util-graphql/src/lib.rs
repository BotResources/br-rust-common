//! The **GraphQL/REST edge kit** every BotResources service imports. Tier
//! `util` — technical wrappers (types / functions / traits), **no domain**: no
//! aggregate, no event, no projector, no `CommandResult`. It supports the
//! collaborative-pure doctrine; it embodies none of it.
//!
//! Everything sits behind the **`graphql`** feature (off by default) — enabling
//! it pulls `async-graphql` + `axum` + `br-core-values`. A crate can depend on
//! this one transitively without taking that weight (or the async-graphql
//! version coupling) unless it actually wires an edge.
//!
//! ## What it provides (all `feature = "graphql"`)
//!
//! - **3-layer error mapping** — [`ErrorCode`] (the cross-service code contract
//!   the frontends bind to) + [`EdgeError`] (domain-error → app-error) with two
//!   render edges: `into_gql` (an `async_graphql::Error` with `extensions.code`)
//!   and `IntoResponse` (the mirrored REST JSON body). Internal-fault detail is
//!   logged, never returned.
//! - **[`Affordance`]** — `{ action, allowed, reason_code }`, the dumb-frontend
//!   contract: a blocked affordance must carry a code (no silent denial).
//! - **[`MutationResult`]** — the success ack; a mutation returns this or an
//!   [`EdgeError`], never a DTO (R1, collaborative-pure).
//! - **[`Connection`] / [`Edge`] / [`PageInfo`]** — reusable cursor pagination.
//! - **[`SubscriptionPayload`]** — the collaborative-pure push: event + fresh
//!   entity + recalculated affordances, so a client folds it without refetching.
//! - **Fallible VO wrappers** ([`values`]) — `TryFrom`/seam conversions over
//!   `br-core-values` that **fail with a typed code, never coerce**
//!   (`LOCALE_UNKNOWN`, `MONEY_OUT_OF_RANGE`, `PRIMARY_CONTENT_MISSING`).
//!
//! ## Codes, not language
//!
//! Every code this crate emits — the [`ErrorCode`] strings, an [`Affordance`]
//! `reason_code`, a conversion code — is a **stable key, never UI prose**. The
//! human text and its i18n (EN/FR/JP) live at the client edge, keyed on the code.

#![cfg(feature = "graphql")]

mod affordance;
mod error;
mod mutation;
mod pagination;
mod subscription;
pub mod values;

pub use affordance::Affordance;
pub use error::{EdgeError, ErrorCode};
pub use mutation::MutationResult;
pub use pagination::{Connection, Edge, PageInfo};
pub use subscription::SubscriptionPayload;
