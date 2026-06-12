use br_core_scope::{ScopeKey, ServiceKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RegistryEvent {
    ServiceRegistered {
        service: ServiceKey,
        label_key: String,
        description_key: String,
    },
    ScopeRegistered {
        key: ScopeKey,
        owning_service: ServiceKey,
        label_key: String,
        description_key: String,
        platform_only: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CommandResult {
    pub events: Vec<RegistryEvent>,
    pub warnings: Vec<RegistryWarning>,
}

impl CommandResult {
    pub fn from_events(events: Vec<RegistryEvent>) -> Self {
        Self {
            events,
            warnings: Vec::new(),
        }
    }

    pub fn is_noop(&self) -> bool {
        self.events.is_empty() && self.warnings.is_empty()
    }
}

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
