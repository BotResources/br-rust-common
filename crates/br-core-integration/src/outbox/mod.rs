//! Transactional outbox: stage an integration message in the **same
//! transaction** as the domain write, then publish it with a **subscribe-driven**
//! relay that doubles as the crash-recovery sweep.
//!
//! ## Why
//!
//! A critical integration event published best-effort after a commit can be
//! lost in the window between the domain commit and the bus publish (a crash, a
//! broker blip) — and a lost event in a choreography is the hardest bug to
//! diagnose, because nothing errors: the producer succeeded, the consumer simply
//! never heard. The outbox closes that window: the message becomes durable
//! atomically with the state it announces, and a relay guarantees it is
//! eventually published.
//!
//! ## Subscribe-driven, never polled
//!
//! `stage` fires `pg_notify` on the table's channel **inside the caller's
//! transaction** (delivered at commit, never on rollback), and the relay's
//! `run` loop `LISTEN`s on that channel. The relay is woken exactly when a row is
//! durably committed — it does **not** read the outbox on a blind timer. When the
//! outbox is clean and no retry is owed it parks at zero CPU and issues zero DB
//! traffic (BR's never-poll rule).
//!
//! ## Shape
//!
//! - [`OutboxStatus`] / [`Transition`] / [`next_after_attempt`]
//!   — the **pure** state machine (no feature, always compiled and spec-tested).
//! - [`OutboxRecord`] — the staged message as a typed value (pure).
//! - [`verify_consumer`] — the opt-in receiver-online precheck. **Ungated** (it
//!   touches only `async_nats`, no `sqlx`): a service that issues a critical
//!   command needs it whether or not it stages outbox rows.
//! - **`outbox` feature** (pulls `sqlx`): `stage` / `OutboxStore` — the
//!   same-transaction insert (with the notify-after-commit wake) and the relay's
//!   queries; `OutboxRelay` — the subscribe-driven / recovery publisher that
//!   processes one row per short transaction, with a `RelayHealth` signal for
//!   the consuming service's readiness gate.
//!
//! The pure half stays DB-free so a consumer that only needs the status type, the
//! precheck, or that wires its own persistence, does not pull `sqlx`. Enable the
//! `outbox` feature for the Postgres store + relay.
//!
//! ## The table is a declared object (the lib never auto-provisions)
//!
//! The store assumes the outbox table already exists — the consuming service's
//! migrations own it. The canonical DDL (the contract the store binds to):
//!
//! ```sql
//! CREATE TABLE integration_outbox (
//!     id           UUID        PRIMARY KEY,            -- UUIDv7, creator-supplied
//!     subject      TEXT        NOT NULL,
//!     payload      JSONB       NOT NULL,
//!     status       TEXT        NOT NULL DEFAULT 'PENDING',
//!     attempts     BIGINT      NOT NULL DEFAULT 0,
//!     last_error   TEXT,
//!     created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
//!     published_at TIMESTAMPTZ
//! );
//! -- the relay's per-row pick-up query filters on status and orders by id:
//! CREATE INDEX integration_outbox_pending_idx
//!     ON integration_outbox (id) WHERE status = 'PENDING';
//! ```
//!
//! A missing table is a fail-loud sqlx error, never an on-demand `CREATE TABLE`.

mod record;
mod retry;
mod status;
mod verify;

pub use record::OutboxRecord;
pub use retry::{
    FailureClass, RETRY_BACKOFF_BASE, RETRY_BACKOFF_MAX, classify_failure, retry_backoff,
};
pub use status::{OutboxStatus, Transition, UnknownOutboxStatus, next_after_attempt};
pub use verify::verify_consumer;

#[cfg(feature = "outbox")]
mod driver;
#[cfg(feature = "outbox")]
mod health;
#[cfg(feature = "outbox")]
mod relay;
#[cfg(feature = "outbox")]
mod report;
#[cfg(feature = "outbox")]
mod stage;
#[cfg(feature = "outbox")]
mod store;
#[cfg(feature = "outbox")]
mod table_name;

#[cfg(feature = "outbox")]
pub use driver::RelayRunError;
#[cfg(feature = "outbox")]
pub use health::{REASON_NO_STREAM, RelayHealth, RelayHealthReceiver};
#[cfg(feature = "outbox")]
pub use relay::OutboxRelay;
#[cfg(feature = "outbox")]
pub use report::{DEFAULT_MAX_ATTEMPTS, DEFAULT_MAX_MESSAGES, RelayPolicy, RelayReport};
#[cfg(feature = "outbox")]
pub use stage::{stage, stage_into};
#[cfg(feature = "outbox")]
pub use store::{DEFAULT_TABLE, OutboxStore, OutboxStoreError};
