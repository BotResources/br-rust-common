//! The domain events the [`ScopeRegistry`](crate::ScopeRegistry) emits, and the
//! [`CommandResult`] a command returns.
//!
//! These are **domain events** on the domain bus — internal to the Identity BC,
//! unprefixed, past tense, one fact per event, each carrying its full value so a
//! subscriber never has to re-query. They are *not* the integration-bus
//! accepted/rejected confirmations: lowering a declaration verdict to a
//! `br-core-integration` envelope is the application layer's job, not this
//! crate's.
//!
//! ## Granularity
//!
//! A declaration that registers a never-seen service and two of its scopes emits
//! one [`ServiceRegistered`](RegistryEvent::ServiceRegistered) and two
//! [`ScopeRegistered`](RegistryEvent::ScopeRegistered) — one fact each, in the
//! order they were applied. There is deliberately no coarse `RegistryUpdated`:
//! each event states exactly what became true.

use br_core_scope::{ScopeKey, ServiceKey};
use serde::{Deserialize, Serialize};

/// A fact that became true in the [`ScopeRegistry`](crate::ScopeRegistry).
///
/// Past tense, granular, self-contained. Each event carries everything a
/// subscriber needs to fold it into its own view without re-querying — the
/// service's manifest data on registration, the scope's full metadata on
/// registration.
///
/// `#[non_exhaustive]`: a future fact (e.g. a scope being retired) is an
/// additive change; match with a wildcard arm.
///
/// The serde shape is **internally tagged** on `event` and `snake_case`
/// (`{ "event": "scope_registered", … }`), mirroring how `br-core-scope` tags
/// its rejection language — a stable wire contract locked by a wire-format test
/// (a renamed variant or dropped field fails it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RegistryEvent {
    /// A service was registered for the first time — it now appears in the
    /// registry as a first-class entity, before any of its scopes.
    ServiceRegistered {
        /// The service's key (and the `{service}` prefix all its scopes share).
        service: ServiceKey,
        /// i18n key for the service's display label.
        label_key: String,
        /// i18n key for the service's description.
        description_key: String,
    },
    /// A scope was registered under its owning service.
    ScopeRegistered {
        /// The validated permission key.
        key: ScopeKey,
        /// The service that owns the scope.
        owning_service: ServiceKey,
        /// i18n key for the scope's short label.
        label_key: String,
        /// i18n key for the scope's longer description.
        description_key: String,
        /// Whether the scope is reserved for platform-internal use.
        platform_only: bool,
    },
}

/// What a state-changing command returns: the events it produced plus any
/// non-fatal warnings.
///
/// The scope-registration slice has no warning case today, so `warnings` is
/// always empty here — it is carried for forward-compat because the
/// `CommandResult { events, warnings }` shape is part of the command contract,
/// and a later command (e.g. one that flags a deprecated-but-accepted scope)
/// should not have to change the return type. An idempotent no-op returns an
/// *empty* `events` (and so an empty result), never an error.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CommandResult {
    /// The domain events the command produced, in application order.
    pub events: Vec<RegistryEvent>,
    /// Non-fatal warnings produced alongside the events (none in this slice).
    pub warnings: Vec<RegistryWarning>,
}

impl CommandResult {
    /// A result carrying `events` and no warnings — the only shape this slice
    /// produces.
    pub fn from_events(events: Vec<RegistryEvent>) -> Self {
        Self {
            events,
            warnings: Vec::new(),
        }
    }

    /// Whether the command changed nothing (no events, no warnings) — the
    /// idempotent no-op case.
    pub fn is_noop(&self) -> bool {
        self.events.is_empty() && self.warnings.is_empty()
    }
}

/// A non-fatal warning a command may attach to its [`CommandResult`].
///
/// Empty in the scope-registration slice — declared as an `#[non_exhaustive]`
/// enum (rather than, say, a string) so the first real warning is a typed,
/// codes-not-language variant, never UI prose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum RegistryWarning {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_events_carries_events_and_no_warnings() {
        let result = CommandResult::from_events(vec![RegistryEvent::ServiceRegistered {
            service: ServiceKey::new("notifier").unwrap(),
            label_key: "l".to_string(),
            description_key: "d".to_string(),
        }]);
        assert_eq!(result.events.len(), 1);
        assert!(result.warnings.is_empty());
        assert!(!result.is_noop());
    }

    #[test]
    fn default_is_the_noop_result() {
        let result = CommandResult::default();
        assert!(result.is_noop());
        assert!(result.events.is_empty());
    }

    // The event shape is a public contract a subscriber folds: a granular event
    // carries its full value and round-trips on the wire (a dropped field is a
    // bug). The shape is internally tagged on `event`, snake_case, with every
    // field at the top level. Lock both variants so a renamed variant/field or a
    // changed tag fails a test.
    #[test]
    fn scope_registered_event_locks_its_wire_shape_and_round_trips() {
        let event = RegistryEvent::ScopeRegistered {
            key: ScopeKey::new("notifier:admin").unwrap(),
            owning_service: ServiceKey::new("notifier").unwrap(),
            label_key: "scope.admin.label".to_string(),
            description_key: "scope.admin.desc".to_string(),
            platform_only: true,
        };
        let json = serde_json::to_value(&event).unwrap();
        // Internally tagged on `event`, snake_case; fields are top-level and the
        // embedded keys serialize as bare strings.
        assert_eq!(json["event"], "scope_registered");
        assert_eq!(json["key"], "notifier:admin");
        assert_eq!(json["owning_service"], "notifier");
        assert_eq!(json["label_key"], "scope.admin.label");
        assert_eq!(json["description_key"], "scope.admin.desc");
        assert_eq!(json["platform_only"], true);
        let back: RegistryEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }

    #[test]
    fn service_registered_event_locks_its_wire_shape_and_round_trips() {
        let event = RegistryEvent::ServiceRegistered {
            service: ServiceKey::new("notifier").unwrap(),
            label_key: "service.label".to_string(),
            description_key: "service.desc".to_string(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["event"], "service_registered");
        assert_eq!(json["service"], "notifier");
        assert_eq!(json["label_key"], "service.label");
        assert_eq!(json["description_key"], "service.desc");
        let back: RegistryEvent = serde_json::from_value(json).unwrap();
        assert_eq!(back, event);
    }
}
