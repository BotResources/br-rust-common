//! Minimal kernel types shared across BotResources Rust services.
//!
//! Today this crate exposes typed ID wrappers ([`UserId`],
//! [`ServiceAccountId`]) and the [`Actor`] that ties them together (a human or
//! a machine identity). Keep it intentionally small: only add types that are
//! genuinely universal across every service.
//!
//! The wrappers are deliberately *not* `Deref<Target = Uuid>`: deref coercion
//! would silently coerce a [`UserId`] into a `&Uuid` anywhere a `&Uuid` is
//! expected (UUID-keyed maps, SQL binds, `&Uuid`-taking functions), reopening
//! the "two UUIDs are interchangeable" hole this crate exists to close. To
//! reach the inner value, call [`UserId::as_uuid`] / [`ServiceAccountId::as_uuid`]
//! or use the `AsRef<Uuid>` impls — explicit, never implicit.
//!
//! The two id types are mutually non-interchangeable at compile time:
//!
//! ```compile_fail
//! use br_core_kernel::{ServiceAccountId, UserId};
//! use uuid::Uuid;
//!
//! fn takes_user(_: UserId) {}
//!
//! let id = ServiceAccountId::from(Uuid::nil());
//! takes_user(id); // a ServiceAccountId is not a UserId
//! ```

use std::fmt;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod actor;

pub use actor::Actor;

/// Unique identifier of a human user.
///
/// Used in `Passport::Human`, domain events, and any reference to a human
/// actor. Compile-time distinct from [`ServiceAccountId`] and from a bare
/// `Uuid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UserId(pub Uuid);

impl UserId {
    /// Returns the inner `Uuid` by value (`Uuid` is `Copy`).
    ///
    /// Prefer this (or [`AsRef<Uuid>`]) over any implicit coercion: the wrapper
    /// intentionally does not deref to `Uuid`, so reaching the raw value is
    /// always an explicit, greppable call site.
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for UserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for UserId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl AsRef<Uuid> for UserId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl From<UserId> for Uuid {
    fn from(id: UserId) -> Self {
        id.0
    }
}

/// Unique identifier of a service account (machine identity).
///
/// Used in `Passport::Service`, integration events, and any cross-BC
/// reference to a machine identity. Compile-time distinct from [`UserId`] and
/// from a bare `Uuid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ServiceAccountId(pub Uuid);

impl ServiceAccountId {
    /// Returns the inner `Uuid` by value (`Uuid` is `Copy`).
    ///
    /// Prefer this (or [`AsRef<Uuid>`]) over any implicit coercion: the wrapper
    /// intentionally does not deref to `Uuid`, so reaching the raw value is
    /// always an explicit, greppable call site.
    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl fmt::Display for ServiceAccountId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<Uuid> for ServiceAccountId {
    fn from(id: Uuid) -> Self {
        Self(id)
    }
}

impl AsRef<Uuid> for ServiceAccountId {
    fn as_ref(&self) -> &Uuid {
        &self.0
    }
}

impl From<ServiceAccountId> for Uuid {
    fn from(id: ServiceAccountId) -> Self {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // ─── UserId ───────────────────────────────────────

    #[test]
    fn user_id_display_delegates_to_uuid() {
        let uuid = Uuid::nil();
        let id = UserId(uuid);
        assert_eq!(id.to_string(), uuid.to_string());
    }

    #[test]
    fn user_id_from_uuid() {
        let uuid = Uuid::nil();
        let id = UserId::from(uuid);
        assert_eq!(id.0, uuid);
    }

    #[test]
    fn user_id_as_uuid_returns_inner() {
        let uuid = Uuid::from_u128(42);
        let id = UserId(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[test]
    fn user_id_as_ref_returns_inner() {
        let uuid = Uuid::from_u128(42);
        let id = UserId(uuid);
        let inner: &Uuid = id.as_ref();
        assert_eq!(*inner, uuid);
    }

    #[test]
    fn user_id_into_uuid() {
        let uuid = Uuid::from_u128(42);
        let id = UserId(uuid);
        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn user_id_serde_roundtrip() {
        let id = UserId(Uuid::nil());
        let json = serde_json::to_string(&id).unwrap();
        let back: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn user_id_wire_format_is_plain_uuid_string() {
        let uuid = Uuid::from_u128(0x1234_5678);
        let id = UserId::from(uuid);
        // `#[serde(transparent)]` locks the wire format to a bare UUID string.
        // The default newtype encoding already produced this shape; the
        // attribute + this test make it contractual instead of incidental.
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{uuid}\""));
    }

    #[test]
    fn user_id_deserializes_from_plain_uuid_string() {
        let uuid = Uuid::from_u128(0x1234_5678);
        let id: UserId = serde_json::from_str(&format!("\"{uuid}\"")).unwrap();
        assert_eq!(id, UserId::from(uuid));
    }

    #[test]
    fn user_id_equality() {
        let uuid = Uuid::nil();
        assert_eq!(UserId(uuid), UserId(uuid));
    }

    #[test]
    fn user_id_inequality() {
        let a = UserId(Uuid::nil());
        let b = UserId(Uuid::from_u128(1));
        assert_ne!(a, b);
    }

    #[test]
    fn user_id_hash_consistency() {
        use std::collections::HashSet;
        let uuid = Uuid::nil();
        let mut set = HashSet::new();
        set.insert(UserId(uuid));
        assert!(set.contains(&UserId(uuid)));
    }

    // ─── ServiceAccountId ─────────────────────────────

    #[test]
    fn service_account_id_display_delegates_to_uuid() {
        let uuid = Uuid::nil();
        let id = ServiceAccountId(uuid);
        assert_eq!(id.to_string(), uuid.to_string());
    }

    #[test]
    fn service_account_id_from_uuid() {
        let uuid = Uuid::nil();
        let id = ServiceAccountId::from(uuid);
        assert_eq!(id.0, uuid);
    }

    #[test]
    fn service_account_id_as_uuid_returns_inner() {
        let uuid = Uuid::from_u128(42);
        let id = ServiceAccountId(uuid);
        assert_eq!(id.as_uuid(), uuid);
    }

    #[test]
    fn service_account_id_as_ref_returns_inner() {
        let uuid = Uuid::from_u128(42);
        let id = ServiceAccountId(uuid);
        let inner: &Uuid = id.as_ref();
        assert_eq!(*inner, uuid);
    }

    #[test]
    fn service_account_id_into_uuid() {
        let uuid = Uuid::from_u128(42);
        let id = ServiceAccountId(uuid);
        assert_eq!(Uuid::from(id), uuid);
    }

    #[test]
    fn service_account_id_serde_roundtrip() {
        let id = ServiceAccountId(Uuid::nil());
        let json = serde_json::to_string(&id).unwrap();
        let back: ServiceAccountId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn service_account_id_wire_format_is_plain_uuid_string() {
        let uuid = Uuid::from_u128(0x1234_5678);
        let id = ServiceAccountId::from(uuid);
        // `#[serde(transparent)]` locks the wire format to a bare UUID string.
        // The default newtype encoding already produced this shape; the
        // attribute + this test make it contractual instead of incidental.
        assert_eq!(serde_json::to_string(&id).unwrap(), format!("\"{uuid}\""));
    }

    #[test]
    fn service_account_id_deserializes_from_plain_uuid_string() {
        let uuid = Uuid::from_u128(0x1234_5678);
        let id: ServiceAccountId = serde_json::from_str(&format!("\"{uuid}\"")).unwrap();
        assert_eq!(id, ServiceAccountId::from(uuid));
    }

    #[test]
    fn service_account_id_equality() {
        let uuid = Uuid::nil();
        assert_eq!(ServiceAccountId(uuid), ServiceAccountId(uuid));
    }

    #[test]
    fn service_account_id_inequality() {
        let a = ServiceAccountId(Uuid::nil());
        let b = ServiceAccountId(Uuid::from_u128(1));
        assert_ne!(a, b);
    }

    #[test]
    fn service_account_id_hash_consistency() {
        use std::collections::HashSet;
        let uuid = Uuid::nil();
        let mut set = HashSet::new();
        set.insert(ServiceAccountId(uuid));
        assert!(set.contains(&ServiceAccountId(uuid)));
    }
}
