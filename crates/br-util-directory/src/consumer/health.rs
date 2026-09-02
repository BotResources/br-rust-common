use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use br_util_nats_fabric::{WatchHealth, WatchHealthReceiver};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DirectoryStream {
    Users,
    Groups,
    ServiceAccounts,
}

struct Activation {
    generation: u64,
    streams: BTreeMap<DirectoryStream, WatchHealth>,
}

#[derive(Clone)]
pub(crate) struct ProjectorHealth {
    sender: Arc<watch::Sender<WatchHealth>>,
    activation: Arc<Mutex<Activation>>,
}

impl ProjectorHealth {
    pub(crate) fn new() -> Self {
        Self {
            sender: Arc::new(watch::Sender::new(WatchHealth::Degraded)),
            activation: Arc::new(Mutex::new(Activation {
                generation: 0,
                streams: BTreeMap::new(),
            })),
        }
    }

    pub(crate) fn receiver(&self) -> WatchHealthReceiver {
        self.sender.subscribe()
    }

    pub(crate) fn activate(&self, active: &[DirectoryStream]) -> ActiveStreams {
        let mut generation = 0;
        self.mutate(|activation| {
            activation.generation += 1;
            generation = activation.generation;
            activation.streams = active
                .iter()
                .map(|stream| (*stream, WatchHealth::Degraded))
                .collect();
        });
        ActiveStreams {
            health: self.clone(),
            generation,
        }
    }

    pub(crate) fn set(&self, stream: DirectoryStream, health: WatchHealth) {
        self.mutate(|activation| {
            if let Some(slot) = activation.streams.get_mut(&stream) {
                *slot = health;
            }
        });
    }

    fn deactivate(&self, generation: u64) {
        self.mutate(|activation| {
            if activation.generation == generation {
                activation.streams.clear();
            }
        });
    }

    fn mutate(&self, change: impl FnOnce(&mut Activation)) {
        let composed = {
            let mut activation = self.activation.lock().expect("projector health lock");
            change(&mut activation);
            worst_of(&activation.streams)
        };
        self.sender.send_if_modified(|current| {
            if *current == composed {
                false
            } else {
                *current = composed;
                true
            }
        });
    }
}

#[must_use]
pub(crate) struct ActiveStreams {
    health: ProjectorHealth,
    generation: u64,
}

impl Drop for ActiveStreams {
    fn drop(&mut self) {
        self.health.deactivate(self.generation);
    }
}

fn worst_of(streams: &BTreeMap<DirectoryStream, WatchHealth>) -> WatchHealth {
    if !streams.is_empty() && streams.values().all(|h| *h == WatchHealth::Healthy) {
        WatchHealth::Healthy
    } else {
        WatchHealth::Degraded
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_is_degraded_before_any_stream_is_active() {
        let health = ProjectorHealth::new();
        assert_eq!(*health.receiver().borrow(), WatchHealth::Degraded);
    }

    #[test]
    fn health_turns_healthy_only_once_every_active_stream_is_healthy() {
        let health = ProjectorHealth::new();
        let receiver = health.receiver();
        let _active = health.activate(&[DirectoryStream::Users, DirectoryStream::Groups]);

        health.set(DirectoryStream::Users, WatchHealth::Healthy);
        assert_eq!(*receiver.borrow(), WatchHealth::Degraded);

        health.set(DirectoryStream::Groups, WatchHealth::Healthy);
        assert_eq!(*receiver.borrow(), WatchHealth::Healthy);
    }

    #[test]
    fn one_degraded_stream_degrades_the_composition() {
        let health = ProjectorHealth::new();
        let _active = health.activate(&[DirectoryStream::Users, DirectoryStream::ServiceAccounts]);
        health.set(DirectoryStream::Users, WatchHealth::Healthy);
        health.set(DirectoryStream::ServiceAccounts, WatchHealth::Healthy);
        health.set(DirectoryStream::ServiceAccounts, WatchHealth::Degraded);
        assert_eq!(*health.receiver().borrow(), WatchHealth::Degraded);
    }

    #[test]
    fn an_inactive_stream_never_holds_the_composition_back() {
        let health = ProjectorHealth::new();
        let _active = health.activate(&[DirectoryStream::Users]);
        health.set(DirectoryStream::Groups, WatchHealth::Degraded);
        health.set(DirectoryStream::Users, WatchHealth::Healthy);
        assert_eq!(*health.receiver().borrow(), WatchHealth::Healthy);
    }

    #[test]
    fn dropping_the_activation_guard_returns_the_composition_to_degraded() {
        let health = ProjectorHealth::new();
        let active = health.activate(&[DirectoryStream::Users]);
        health.set(DirectoryStream::Users, WatchHealth::Healthy);
        drop(active);
        assert_eq!(*health.receiver().borrow(), WatchHealth::Degraded);
    }

    #[test]
    fn a_guard_dropped_while_unwinding_still_returns_the_composition_to_degraded() {
        let health = ProjectorHealth::new();
        let panicking = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _active = health.activate(&[DirectoryStream::Users]);
            health.set(DirectoryStream::Users, WatchHealth::Healthy);
            assert_eq!(*health.receiver().borrow(), WatchHealth::Healthy);
            panic!("the watch is torn down");
        }));
        assert!(panicking.is_err());
        assert_eq!(*health.receiver().borrow(), WatchHealth::Degraded);
    }

    #[test]
    fn a_superseded_guard_dropped_late_leaves_the_current_activation_alone() {
        let health = ProjectorHealth::new();
        let superseded = health.activate(&[DirectoryStream::Users]);
        let _current = health.activate(&[DirectoryStream::Users]);
        health.set(DirectoryStream::Users, WatchHealth::Healthy);

        drop(superseded);

        assert_eq!(*health.receiver().borrow(), WatchHealth::Healthy);
    }

    #[test]
    fn the_current_guard_still_deactivates_after_a_superseded_one_was_dropped() {
        let health = ProjectorHealth::new();
        let superseded = health.activate(&[DirectoryStream::Users]);
        let current = health.activate(&[DirectoryStream::Users]);
        health.set(DirectoryStream::Users, WatchHealth::Healthy);
        drop(superseded);

        drop(current);

        assert_eq!(*health.receiver().borrow(), WatchHealth::Degraded);
    }
}
