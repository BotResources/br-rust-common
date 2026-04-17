//! Minimal kernel types shared across BotResources Rust services.
//!
//! Today this crate exposes typed ID wrappers. Keep it intentionally small:
//! only add types that are genuinely universal across every service.

use std::fmt;
use std::ops::Deref;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

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

impl Deref for UserId {
    type Target = Uuid;
    fn deref(&self) -> &Uuid {
        &self.0
    }
}

/// Unique identifier of a service account (machine identity).
///
/// Used in `Passport::Service`, integration events, and any cross-BC
/// reference to a machine identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceAccountId(pub Uuid);

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

impl Deref for ServiceAccountId {
    type Target = Uuid;
    fn deref(&self) -> &Uuid {
        &self.0
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
    fn user_id_deref_to_uuid() {
        let uuid = Uuid::nil();
        let id = UserId(uuid);
        let inner: &Uuid = &id;
        assert_eq!(*inner, uuid);
    }

    #[test]
    fn user_id_serde_roundtrip() {
        let id = UserId(Uuid::nil());
        let json = serde_json::to_string(&id).unwrap();
        let back: UserId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
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
    fn service_account_id_deref_to_uuid() {
        let uuid = Uuid::nil();
        let id = ServiceAccountId(uuid);
        let inner: &Uuid = &id;
        assert_eq!(*inner, uuid);
    }

    #[test]
    fn service_account_id_serde_roundtrip() {
        let id = ServiceAccountId(Uuid::nil());
        let json = serde_json::to_string(&id).unwrap();
        let back: ServiceAccountId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
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

    // ─── Cross-type: IDs are not interchangeable ──────

    #[test]
    fn user_id_and_service_account_id_are_distinct_types() {
        let uuid = Uuid::nil();
        let user = UserId(uuid);
        let service = ServiceAccountId(uuid);
        // Same inner UUID but different types — this is a compile-time guarantee.
        // We verify the Display output is identical (both delegate to Uuid).
        assert_eq!(user.to_string(), service.to_string());
    }
}
