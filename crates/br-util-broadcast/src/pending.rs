#[derive(Debug, Clone)]
pub struct PendingBroadcast<T> {
    pub(crate) events: Vec<T>,
}

impl<T> PendingBroadcast<T> {
    #[must_use]
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    #[must_use]
    pub fn from_events(events: Vec<T>) -> Self {
        Self { events }
    }

    pub fn push(&mut self, event: T) {
        self.events.push(event);
    }

    pub fn extend(&mut self, events: impl IntoIterator<Item = T>) {
        self.events.extend(events);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.events.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

impl<T> Default for PendingBroadcast<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> FromIterator<T> for PendingBroadcast<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self {
            events: iter.into_iter().collect(),
        }
    }
}
